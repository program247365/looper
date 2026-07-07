use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use color_eyre::eyre::{eyre, Result, WrapErr};
use diesel::{prelude::*, sqlite::SqliteConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use directories::ProjectDirs;

use crate::{playback_input::PlaybackInput, plugin::TrackInfo, schema::played_tracks};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[derive(Clone)]
pub struct Storage {
    db_path: PathBuf,
}

pub type SharedStorage = Arc<Mutex<Storage>>;

/// Surfaced to the TUI when the configured sync database (e.g. iCloud Drive)
/// can't be opened and we silently fall back to the local DB. The TUI shows
/// this as a persistent banner so the user knows sync is off until they grant
/// access — `eprintln!` from inside the alternate-screen TUI is invisible.
#[derive(Clone, Debug)]
pub struct SyncWarning {
    pub attempted_path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistorySortField {
    TimePlayed,
    LastPlayed,
    Favorites,
    Platform,
    Title,
    PlayCount,
}

impl HistorySortField {
    pub fn label(self) -> &'static str {
        match self {
            Self::TimePlayed => "Time Played",
            Self::LastPlayed => "Last Played",
            Self::Favorites => "Favorites",
            Self::Platform => "Platform",
            Self::Title => "Title",
            Self::PlayCount => "Times Played",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::TimePlayed => Self::LastPlayed,
            Self::LastPlayed => Self::Favorites,
            Self::Favorites => Self::Platform,
            Self::Platform => Self::Title,
            Self::Title => Self::PlayCount,
            Self::PlayCount => Self::TimePlayed,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::TimePlayed => Self::PlayCount,
            Self::LastPlayed => Self::TimePlayed,
            Self::Favorites => Self::LastPlayed,
            Self::Platform => Self::Favorites,
            Self::Title => Self::Platform,
            Self::PlayCount => Self::Title,
        }
    }
}

/// Whether a history row is a single track or a whole playlist/album. Stored
/// as TEXT in SQLite; anything unrecognized reads back as `Track` so a DB
/// touched by a newer looper can't break an older one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordKind {
    Track,
    Collection,
}

impl RecordKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Collection => "collection",
        }
    }

    fn from_db(value: &str) -> Self {
        match value {
            "collection" => Self::Collection,
            _ => Self::Track,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRow {
    pub track_key: String,
    pub replay_target: String,
    pub title: String,
    pub platform: String,
    pub is_favorite: bool,
    pub play_count: i64,
    pub total_play_seconds: i64,
    pub first_played_at: i64,
    pub last_played_at: i64,
    pub last_played_computer: String,
    pub kind: RecordKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackRecord {
    pub track_key: String,
    pub replay_target: String,
    pub title: String,
    pub platform: String,
    pub kind: RecordKind,
}

#[derive(Insertable)]
#[diesel(table_name = played_tracks)]
struct NewPlayedTrack<'a> {
    track_key: &'a str,
    replay_target: &'a str,
    title: &'a str,
    platform: &'a str,
    is_favorite: bool,
    play_count: i64,
    total_play_seconds: i64,
    first_played_at: i64,
    last_played_at: i64,
    last_played_computer: &'a str,
    kind: &'a str,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = played_tracks)]
struct PlayedTrackRow {
    track_key: String,
    replay_target: String,
    title: String,
    platform: String,
    is_favorite: bool,
    play_count: i64,
    total_play_seconds: i64,
    first_played_at: i64,
    last_played_at: i64,
    last_played_computer: String,
    kind: String,
}

impl From<PlayedTrackRow> for HistoryRow {
    fn from(row: PlayedTrackRow) -> Self {
        Self {
            track_key: row.track_key,
            replay_target: row.replay_target,
            title: row.title,
            platform: row.platform,
            is_favorite: row.is_favorite,
            play_count: row.play_count,
            total_play_seconds: row.total_play_seconds,
            first_played_at: row.first_played_at,
            last_played_at: row.last_played_at,
            last_played_computer: row.last_played_computer,
            kind: RecordKind::from_db(&row.kind),
        }
    }
}

impl Storage {
    /// Opens the local working DB (always at `default_db_path()`), pulling from
    /// the configured replica first if one is set. The replica is treated as a
    /// passive sync target — looper never reads or writes it during normal
    /// operation. Replica failures (TCC denial, iCloud eviction, missing
    /// network) are non-fatal: the local DB still opens and a `SyncWarning` is
    /// surfaced for the TUI banner.
    pub fn open_and_migrate() -> Result<(Self, Option<SyncWarning>)> {
        let local_path = default_db_path()?;

        let sync_warning = match read_replica_path() {
            Some(replica_path) => match try_pull_from_replica(&replica_path, &local_path) {
                Ok(_) => None,
                Err(err) => Some(SyncWarning {
                    attempted_path: replica_path,
                    reason: err.to_string(),
                }),
            },
            None => {
                // One-time legacy migration for users upgrading from v0.5.4 and earlier:
                // their history lives in the old iCloud auto-detected path. Pull silently
                // if that path exists and the local DB has no newer data.
                #[cfg(target_os = "macos")]
                if let Some(legacy) = legacy_icloud_db_path() {
                    let _ = try_pull_from_replica(&legacy, &local_path);
                }
                None
            }
        };

        let storage = Self::open_and_migrate_at(local_path)?;
        Ok((storage, sync_warning))
    }

    pub fn shared(self) -> SharedStorage {
        Arc::new(Mutex::new(self))
    }

    pub fn open_and_migrate_at(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).wrap_err("failed to create looper data directory")?;
        }

        let mut connection = establish_connection(&db_path)?;
        enable_wal_mode(&mut connection)?;
        connection
            .run_pending_migrations(MIGRATIONS)
            .map_err(|err| eyre!("failed to run looper database migrations: {err}"))?;

        Ok(Self { db_path })
    }

    /// Pushes the live local DB to the configured replica. No-op when no
    /// replica is configured. Checkpoints the WAL first so the copied file is
    /// self-contained.
    pub fn push_replica(&self) -> Result<()> {
        let Some(replica_path) = read_replica_path() else {
            return Ok(());
        };
        try_push_to_replica(&self.db_path, &replica_path)
    }

    pub fn record_play(&self, record: &TrackRecord) -> Result<()> {
        use crate::schema::played_tracks::dsl as tracks;

        let now = unix_timestamp();
        let computer = computer_name();
        let mut connection = establish_connection(&self.db_path)?;
        connection.transaction::<_, diesel::result::Error, _>(|conn| {
            let existing = tracks::played_tracks
                .filter(tracks::track_key.eq(&record.track_key))
                .select(PlayedTrackRow::as_select())
                .first::<PlayedTrackRow>(conn)
                .optional()?;

            if let Some(existing) = existing {
                diesel::update(
                    tracks::played_tracks.filter(tracks::track_key.eq(&record.track_key)),
                )
                .set((
                    tracks::replay_target.eq(&record.replay_target),
                    tracks::title.eq(&record.title),
                    tracks::platform.eq(&record.platform),
                    tracks::play_count.eq(existing.play_count + 1),
                    tracks::last_played_at.eq(now),
                    tracks::last_played_computer.eq(&computer),
                    tracks::kind.eq(record.kind.as_str()),
                ))
                .execute(conn)?;
            } else {
                let row = NewPlayedTrack {
                    track_key: &record.track_key,
                    replay_target: &record.replay_target,
                    title: &record.title,
                    platform: &record.platform,
                    is_favorite: false,
                    play_count: 1,
                    total_play_seconds: 0,
                    first_played_at: now,
                    last_played_at: now,
                    last_played_computer: &computer,
                    kind: record.kind.as_str(),
                };
                diesel::insert_into(tracks::played_tracks)
                    .values(&row)
                    .execute(conn)?;
            }

            Ok(())
        })?;

        Ok(())
    }

    pub fn record_playback_time(&self, track_key: &str, played_seconds: i64) -> Result<()> {
        use crate::schema::played_tracks::dsl as tracks;

        if played_seconds <= 0 {
            return Ok(());
        }

        let mut connection = establish_connection(&self.db_path)?;
        connection.transaction::<_, diesel::result::Error, _>(|conn| {
            let existing_seconds = tracks::played_tracks
                .filter(tracks::track_key.eq(track_key))
                .select(tracks::total_play_seconds)
                .first::<i64>(conn)?;

            diesel::update(tracks::played_tracks.filter(tracks::track_key.eq(track_key)))
                .set(tracks::total_play_seconds.eq(existing_seconds + played_seconds))
                .execute(conn)?;
            Ok(())
        })?;

        Ok(())
    }

    pub fn toggle_favorite(&self, track_key: &str) -> Result<bool> {
        use crate::schema::played_tracks::dsl as tracks;

        let mut connection = establish_connection(&self.db_path)?;
        connection
            .transaction::<_, diesel::result::Error, _>(|conn| {
                let current = tracks::played_tracks
                    .filter(tracks::track_key.eq(track_key))
                    .select(tracks::is_favorite)
                    .first::<bool>(conn)?;
                let next = !current;
                diesel::update(tracks::played_tracks.filter(tracks::track_key.eq(track_key)))
                    .set(tracks::is_favorite.eq(next))
                    .execute(conn)?;
                Ok(next)
            })
            .map_err(Into::into)
    }

    pub fn list_history(
        &self,
        sort_field: HistorySortField,
        descending: bool,
    ) -> Result<Vec<HistoryRow>> {
        use crate::schema::played_tracks::dsl as tracks;

        let mut connection = establish_connection(&self.db_path)?;
        let mut rows = tracks::played_tracks
            .select(PlayedTrackRow::as_select())
            .load::<PlayedTrackRow>(&mut connection)?
            .into_iter()
            .map(HistoryRow::from)
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| compare_history_rows(left, right, sort_field));
        if descending {
            rows.reverse();
        }

        Ok(rows)
    }

    pub fn favorite_for(&self, track_key: &str) -> Result<bool> {
        use crate::schema::played_tracks::dsl as tracks;

        let mut connection = establish_connection(&self.db_path)?;
        let favorite = tracks::played_tracks
            .filter(tracks::track_key.eq(track_key))
            .select(tracks::is_favorite)
            .first::<bool>(&mut connection)
            .optional()?
            .unwrap_or(false);
        Ok(favorite)
    }

    pub fn title_for_replay_target(&self, replay_target: &str) -> Result<Option<String>> {
        use crate::schema::played_tracks::dsl as tracks;

        let mut connection = establish_connection(&self.db_path)?;
        let title = tracks::played_tracks
            .filter(tracks::replay_target.eq(replay_target))
            .select(tracks::title)
            .first::<String>(&mut connection)
            .optional()?;
        Ok(title)
    }

    pub fn delete_by_replay_target(&self, replay_target: &str) -> Result<usize> {
        use crate::schema::played_tracks::dsl as tracks;

        let mut connection = establish_connection(&self.db_path)?;
        let removed = diesel::delete(
            tracks::played_tracks.filter(tracks::replay_target.eq(replay_target)),
        )
        .execute(&mut connection)?;
        Ok(removed)
    }
}

fn compare_history_rows(
    left: &HistoryRow,
    right: &HistoryRow,
    field: HistorySortField,
) -> std::cmp::Ordering {
    match field {
        HistorySortField::LastPlayed => left
            .last_played_at
            .cmp(&right.last_played_at)
            .then_with(|| left.title.cmp(&right.title)),
        HistorySortField::Favorites => left
            .is_favorite
            .cmp(&right.is_favorite)
            .then_with(|| left.last_played_at.cmp(&right.last_played_at))
            .then_with(|| left.title.cmp(&right.title)),
        HistorySortField::TimePlayed => left
            .total_play_seconds
            .cmp(&right.total_play_seconds)
            .then_with(|| left.play_count.cmp(&right.play_count))
            .then_with(|| left.title.cmp(&right.title)),
        HistorySortField::Platform => left
            .platform
            .cmp(&right.platform)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.last_played_at.cmp(&right.last_played_at)),
        HistorySortField::Title => left
            .title
            .cmp(&right.title)
            .then_with(|| left.platform.cmp(&right.platform))
            .then_with(|| left.last_played_at.cmp(&right.last_played_at)),
        HistorySortField::PlayCount => left
            .play_count
            .cmp(&right.play_count)
            .then_with(|| left.title.cmp(&right.title)),
    }
}

pub fn track_record(track: &TrackInfo) -> Result<TrackRecord> {
    if let Some(source_url) = &track.source_url {
        return Ok(TrackRecord {
            track_key: source_url.clone(),
            replay_target: source_url.clone(),
            title: track.title.clone(),
            platform: track
                .service
                .clone()
                .unwrap_or_else(|| "Online".to_string()),
            kind: RecordKind::Track,
        });
    }

    match &track.playback {
        PlaybackInput::File(path) => {
            let canonical = canonical_string(path)?;
            Ok(TrackRecord {
                track_key: canonical.clone(),
                replay_target: canonical,
                title: track.title.clone(),
                platform: "Local".to_string(),
                kind: RecordKind::Track,
            })
        }
        PlaybackInput::HttpStream { url, .. } => Ok(TrackRecord {
            track_key: url.clone(),
            replay_target: url.clone(),
            title: track.title.clone(),
            platform: track
                .service
                .clone()
                .unwrap_or_else(|| "Online".to_string()),
            kind: RecordKind::Track,
        }),
        PlaybackInput::ProcessStdout { .. } => Err(eyre!(
            "cannot derive persistent track identity without a source URL"
        )),
        // Reached only if a Spotify track ever lacks a source_url (it never
        // does today); the URI is a stable, replayable identity regardless.
        PlaybackInput::Spotify { track_uri } => Ok(TrackRecord {
            track_key: track_uri.clone(),
            replay_target: track_uri.clone(),
            title: track.title.clone(),
            platform: track
                .service
                .clone()
                .unwrap_or_else(|| "Spotify".to_string()),
            kind: RecordKind::Track,
        }),
    }
}

/// History record for a whole playlist/album launch. Keyed by the URL the user
/// asked for, so replaying the row re-resolves the collection. Title comes from
/// the collection name the resolver stamped on its tracks; a collection whose
/// resolver gave no name falls back to the URL itself — still replayable.
pub fn collection_record(source_url: &str, tracks: &[TrackInfo]) -> TrackRecord {
    let first = tracks.first();
    TrackRecord {
        track_key: source_url.to_string(),
        replay_target: source_url.to_string(),
        title: first
            .and_then(|track| track.collection.clone())
            .unwrap_or_else(|| source_url.to_string()),
        platform: first
            .and_then(|track| track.service.clone())
            .unwrap_or_else(|| "Online".to_string()),
        kind: RecordKind::Collection,
    }
}

/// Returns the configured replica DB path (sync folder + `looper.sqlite3`),
/// if a sync folder is configured. The replica is the passive copy — looper
/// never reads or writes it during normal operation. Pull on startup, push
/// on shutdown.
pub fn read_replica_path() -> Option<PathBuf> {
    read_sync_folder_config().map(|folder| folder.join("looper.sqlite3"))
}

/// Pulls the replica DB into the local DB if the replica looks more recent
/// (or the local DB is missing). "More recent" is determined by
/// `MAX(last_played_at)` — file mtime is unreliable across iCloud sync. The
/// pulled file replaces the local DB atomically via temp + rename. Stale
/// WAL/SHM sidecars are removed; SQLite recreates them on next open.
fn try_pull_from_replica(replica: &Path, local: &Path) -> Result<bool> {
    if !replica.exists() {
        return Ok(false);
    }
    let local_max = if local.exists() {
        db_max_last_played_at(local).unwrap_or(0)
    } else {
        0
    };
    let replica_max = db_max_last_played_at(replica)?;
    if local.exists() && replica_max <= local_max {
        return Ok(false);
    }

    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent).wrap_err("failed to create local data directory")?;
    }
    let tmp = local.with_extension("sqlite3.pull-tmp");
    fs::copy(replica, &tmp).wrap_err("failed to copy replica into pull-tmp")?;
    for ext in ["sqlite3-wal", "sqlite3-shm"] {
        let _ = fs::remove_file(local.with_extension(ext));
    }
    fs::rename(&tmp, local).wrap_err("failed to install pulled replica")?;
    Ok(true)
}

/// Pushes the local DB to the replica path, atomically. Checkpoints the WAL
/// first so the file copy contains all committed writes.
fn try_push_to_replica(local: &Path, replica: &Path) -> Result<()> {
    let _ = checkpoint_db(local);
    if let Some(parent) = replica.parent() {
        fs::create_dir_all(parent).wrap_err("failed to create replica directory")?;
    }
    let tmp = replica.with_extension("sqlite3.push-tmp");
    fs::copy(local, &tmp).wrap_err("failed to copy local DB into push-tmp")?;
    fs::rename(&tmp, replica).wrap_err("failed to install replica")?;
    Ok(())
}

/// Reads `MAX(last_played_at)` from a DB. Runs pending migrations first so the
/// column is available even on older DBs.
fn db_max_last_played_at(path: &Path) -> Result<i64> {
    use crate::schema::played_tracks::dsl as tracks;
    let mut conn = establish_connection(path)?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| eyre!("migrate before reading max(last_played_at) failed: {e}"))?;
    let max: Option<i64> = tracks::played_tracks
        .select(diesel::dsl::max(tracks::last_played_at))
        .first(&mut conn)
        .map_err(|e| eyre!("max(last_played_at) query failed: {e}"))?;
    Ok(max.unwrap_or(0))
}

fn checkpoint_db(path: &Path) -> Result<()> {
    use diesel::connection::SimpleConnection;
    let mut conn = establish_connection(path)?;
    conn.batch_execute("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|e| eyre!("WAL checkpoint failed: {e}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn legacy_icloud_db_path() -> Option<PathBuf> {
    let home = directories::UserDirs::new()?.home_dir().to_path_buf();
    let path = home.join("Library/Mobile Documents/com~apple~CloudDocs/looper/looper.sqlite3");
    if path.exists() { Some(path) } else { None }
}

fn sync_folder_config_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".config").join("looper").join("sync_folder"))
}

/// Returns the user-configured sync folder, if set via `looper config set sync-folder`.
pub fn read_sync_folder_config() -> Option<PathBuf> {
    let raw = fs::read_to_string(sync_folder_config_path()?).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Persists the sync folder so future launches use it instead of iCloud auto-detection.
pub fn write_sync_folder_config(folder: &Path) -> Result<()> {
    let config_path = sync_folder_config_path()
        .ok_or_else(|| eyre!("could not determine config directory"))?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).wrap_err("failed to create looper config directory")?;
    }
    fs::write(&config_path, folder.to_string_lossy().as_ref())
        .wrap_err("failed to write sync folder config")
}

/// Returns the friendly name of this computer.
/// On macOS, uses `scutil --get ComputerName` (e.g. "Kevin's MacBook Pro").
/// Falls back to `hostname` on all platforms.
pub fn computer_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            if out.status.success() {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn enable_wal_mode(conn: &mut SqliteConnection) -> Result<()> {
    use diesel::connection::SimpleConnection;
    conn.batch_execute("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|e| eyre!("failed to enable WAL mode: {e}"))
}

fn default_db_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("sh", "kbr", "looper")
        .ok_or_else(|| eyre!("failed to determine looper data directory"))?;
    Ok(dirs.data_dir().join("looper.sqlite3"))
}

fn establish_connection(db_path: &Path) -> Result<SqliteConnection> {
    SqliteConnection::establish(&db_path.to_string_lossy()).map_err(|err| {
        eyre!(
            "failed to open looper database at {}: {err}",
            db_path.display()
        )
    })
}

fn canonical_string(path: &Path) -> Result<String> {
    Ok(fs::canonicalize(path)
        .wrap_err_with(|| format!("failed to canonicalize path {}", path.display()))?
        .to_string_lossy()
        .into_owned())
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_storage() -> (tempfile::TempDir, Storage) {
        let dir = tempdir().unwrap();
        let storage = Storage::open_and_migrate_at(dir.path().join("history.sqlite3")).unwrap();
        (dir, storage)
    }

    #[test]
    fn computer_name_is_non_empty() {
        assert!(!computer_name().is_empty());
    }

    #[test]
    fn record_play_sets_computer_name() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "local:/test.mp3".into(),
            replay_target: "local:/test.mp3".into(),
            title: "Test".into(),
            platform: "Local".into(),
            kind: RecordKind::Track,
        };
        storage.record_play(&record).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, false)
            .unwrap();
        assert!(!rows[0].last_played_computer.is_empty());
    }

    #[test]
    fn collection_kind_round_trips() {
        let (_dir, storage) = test_storage();
        storage
            .record_play(&TrackRecord {
                track_key: "https://open.spotify.com/album/abc".into(),
                replay_target: "https://open.spotify.com/album/abc".into(),
                title: "Destiny Original Soundtrack".into(),
                platform: "Spotify".into(),
                kind: RecordKind::Collection,
            })
            .unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "spotify:track:xyz".into(),
                replay_target: "spotify:track:xyz".into(),
                title: "The Path".into(),
                platform: "Spotify".into(),
                kind: RecordKind::Track,
            })
            .unwrap();

        let rows = storage
            .list_history(HistorySortField::Title, false)
            .unwrap();
        assert_eq!(rows[0].kind, RecordKind::Collection);
        assert_eq!(rows[1].kind, RecordKind::Track);
    }

    #[test]
    fn collection_record_uses_collection_title_with_url_fallback() {
        let tracks = vec![TrackInfo {
            title: "The Path".into(),
            duration_secs: None,
            playback: PlaybackInput::Spotify {
                track_uri: "spotify:track:xyz".into(),
            },
            source_url: Some("spotify:track:xyz".into()),
            pending_download: None,
            service: Some("Spotify".into()),
            thumbnail_path: None,
            is_live: false,
            collection: Some("Destiny Original Soundtrack".into()),
            artist: None,
        }];

        let record = collection_record("https://open.spotify.com/album/abc", &tracks);
        assert_eq!(record.kind, RecordKind::Collection);
        assert_eq!(record.track_key, "https://open.spotify.com/album/abc");
        assert_eq!(record.title, "Destiny Original Soundtrack");
        assert_eq!(record.platform, "Spotify");

        let mut untitled = tracks;
        untitled[0].collection = None;
        let record = collection_record("https://example.com/playlist", &untitled);
        assert_eq!(record.title, "https://example.com/playlist");
    }

    #[test]
    fn delete_by_replay_target_prunes_dead_entry() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "yt:YmQ7jRgf4f0".into(),
            replay_target: "https://www.youtube.com/watch?v=YmQ7jRgf4f0".into(),
            title: "Claude FM 06-11".into(),
            platform: "YouTube".into(),
            kind: RecordKind::Track,
        };
        storage.record_play(&record).unwrap();

        assert_eq!(
            storage
                .title_for_replay_target(&record.replay_target)
                .unwrap()
                .as_deref(),
            Some("Claude FM 06-11")
        );

        let removed = storage
            .delete_by_replay_target(&record.replay_target)
            .unwrap();
        assert_eq!(removed, 1);
        assert!(storage
            .title_for_replay_target(&record.replay_target)
            .unwrap()
            .is_none());
        assert!(storage
            .list_history(HistorySortField::LastPlayed, false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn pull_replica_skips_when_replica_not_newer() {
        let dir = tempdir().unwrap();
        let local = dir.path().join("local.sqlite3");
        let replica = dir.path().join("replica.sqlite3");

        let storage = Storage::open_and_migrate_at(local.clone()).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "key-x".into(),
                replay_target: "key-x".into(),
                title: "X".into(),
                platform: "Local".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        drop(storage);

        // Snapshot local as the replica — same max(last_played_at).
        fs::copy(&local, &replica).unwrap();

        let pulled = try_pull_from_replica(&replica, &local).unwrap();
        assert!(!pulled, "equal max(last_played_at) should not pull");
    }

    #[test]
    fn pull_replica_replaces_local_when_replica_newer() {
        let dir = tempdir().unwrap();
        let local = dir.path().join("local.sqlite3");
        let replica = dir.path().join("replica.sqlite3");

        // Populate local with one record.
        let storage = Storage::open_and_migrate_at(local.clone()).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "old".into(),
                replay_target: "old".into(),
                title: "Old".into(),
                platform: "Local".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        drop(storage);

        // Replica gets a record with a later last_played_at.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let storage = Storage::open_and_migrate_at(replica.clone()).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "new".into(),
                replay_target: "new".into(),
                title: "New".into(),
                platform: "Local".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        drop(storage);

        let pulled = try_pull_from_replica(&replica, &local).unwrap();
        assert!(pulled, "newer replica should overwrite local");

        let storage = Storage::open_and_migrate_at(local).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, true)
            .unwrap();
        assert!(rows.iter().any(|r| r.track_key == "new"));
        assert!(
            !rows.iter().any(|r| r.track_key == "old"),
            "pull is a copy, not a merge — old local rows are replaced"
        );
    }

    #[test]
    fn push_replica_creates_replica_when_missing() {
        let dir = tempdir().unwrap();
        let local = dir.path().join("local.sqlite3");
        let replica = dir.path().join("nested").join("replica.sqlite3");

        let storage = Storage::open_and_migrate_at(local.clone()).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "k".into(),
                replay_target: "k".into(),
                title: "T".into(),
                platform: "Local".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        drop(storage);

        try_push_to_replica(&local, &replica).unwrap();
        assert!(replica.exists(), "push should create replica file");

        let storage = Storage::open_and_migrate_at(replica).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, true)
            .unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn records_first_play() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "https://example.com/a".into(),
            replay_target: "https://example.com/a".into(),
            title: "A".into(),
            platform: "YouTube".into(),
            kind: RecordKind::Track,
        };

        storage.record_play(&record).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, true)
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].play_count, 1);
        assert_eq!(rows[0].total_play_seconds, 0);
        assert_eq!(rows[0].title, "A");
    }

    #[test]
    fn increments_existing_track() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "https://example.com/a".into(),
            replay_target: "https://example.com/a".into(),
            title: "A".into(),
            platform: "YouTube".into(),
            kind: RecordKind::Track,
        };

        storage.record_play(&record).unwrap();
        storage.record_play(&record).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, true)
            .unwrap();

        assert_eq!(rows[0].play_count, 2);
    }

    #[test]
    fn toggles_favorite() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "https://example.com/a".into(),
            replay_target: "https://example.com/a".into(),
            title: "A".into(),
            platform: "YouTube".into(),
            kind: RecordKind::Track,
        };

        storage.record_play(&record).unwrap();
        assert!(!storage.favorite_for(&record.track_key).unwrap());
        assert!(storage.toggle_favorite(&record.track_key).unwrap());
        assert!(storage.favorite_for(&record.track_key).unwrap());
    }

    #[test]
    fn accumulates_playback_time() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "https://example.com/a".into(),
            replay_target: "https://example.com/a".into(),
            title: "A".into(),
            platform: "YouTube".into(),
            kind: RecordKind::Track,
        };

        storage.record_play(&record).unwrap();
        storage.record_playback_time(&record.track_key, 42).unwrap();
        storage.record_playback_time(&record.track_key, 8).unwrap();

        let rows = storage
            .list_history(HistorySortField::TimePlayed, true)
            .unwrap();
        assert_eq!(rows[0].total_play_seconds, 50);
    }

    #[test]
    fn sorts_rows() {
        let (_dir, storage) = test_storage();
        storage
            .record_play(&TrackRecord {
                track_key: "b".into(),
                replay_target: "b".into(),
                title: "Beta".into(),
                platform: "YouTube".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        storage.record_playback_time("b", 15).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "a".into(),
                replay_target: "a".into(),
                title: "Alpha".into(),
                platform: "SoundCloud".into(),
                kind: RecordKind::Track,
            })
            .unwrap();
        storage.record_playback_time("a", 60).unwrap();

        let rows = storage
            .list_history(HistorySortField::TimePlayed, true)
            .unwrap();
        assert_eq!(rows[0].title, "Alpha");
        assert_eq!(rows[1].title, "Beta");
    }

    #[test]
    fn favorites_sort_puts_starred_rows_first() {
        let (_dir, storage) = test_storage();
        for key in ["plain", "starred"] {
            storage
                .record_play(&TrackRecord {
                    track_key: key.into(),
                    replay_target: key.into(),
                    title: key.into(),
                    platform: "YouTube".into(),
                    kind: RecordKind::Track,
                })
                .unwrap();
        }
        storage.toggle_favorite("starred").unwrap();

        let rows = storage
            .list_history(HistorySortField::Favorites, true)
            .unwrap();
        assert_eq!(rows[0].track_key, "starred");
        assert_eq!(rows[1].track_key, "plain");
    }

    #[test]
    fn favorites_sort_orders_starred_group_by_last_played() {
        let row = |favorite: bool, last_played_at: i64| HistoryRow {
            track_key: "k".into(),
            replay_target: "k".into(),
            title: "T".into(),
            platform: "Local".into(),
            is_favorite: favorite,
            play_count: 1,
            total_play_seconds: 0,
            first_played_at: 0,
            last_played_at,
            last_played_computer: String::new(),
            kind: RecordKind::Track,
        };

        let older_star = row(true, 100);
        let newer_star = row(true, 200);
        let plain = row(false, 300);

        // Ascending comparisons; the panel's default descending reverse floats
        // favorites (and recency within them) to the top.
        use std::cmp::Ordering;
        assert_eq!(
            compare_history_rows(&older_star, &newer_star, HistorySortField::Favorites),
            Ordering::Less
        );
        assert_eq!(
            compare_history_rows(&plain, &older_star, HistorySortField::Favorites),
            Ordering::Less
        );
    }
}
