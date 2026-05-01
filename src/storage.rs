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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistorySortField {
    TimePlayed,
    LastPlayed,
    Platform,
    Title,
    PlayCount,
}

impl HistorySortField {
    pub fn label(self) -> &'static str {
        match self {
            Self::TimePlayed => "Time Played",
            Self::LastPlayed => "Last Played",
            Self::Platform => "Platform",
            Self::Title => "Title",
            Self::PlayCount => "Times Played",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::TimePlayed => Self::LastPlayed,
            Self::LastPlayed => Self::Platform,
            Self::Platform => Self::Title,
            Self::Title => Self::PlayCount,
            Self::PlayCount => Self::TimePlayed,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::TimePlayed => Self::PlayCount,
            Self::LastPlayed => Self::TimePlayed,
            Self::Platform => Self::LastPlayed,
            Self::Title => Self::Platform,
            Self::PlayCount => Self::Title,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackRecord {
    pub track_key: String,
    pub replay_target: String,
    pub title: String,
    pub platform: String,
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
        }
    }
}

impl Storage {
    pub fn open_and_migrate() -> Result<Self> {
        let new_path = resolve_db_path()?;
        let old_path = default_db_path()?;

        // Try the resolved path (iCloud / configured folder / default).
        // If access is denied — common on a fresh macOS machine where the terminal
        // hasn't been granted Full Disk Access — fall back to the local default and
        // print a one-time actionable message so the user knows how to fix it.
        let storage = match Self::open_and_migrate_at(new_path.clone()) {
            Ok(s) => s,
            Err(err) if new_path != old_path => {
                eprintln!(
                    "looper: cannot open sync database at {path}\n  \
                     Reason: {err}\n  \
                     Fix:    System Settings → Privacy & Security → Full Disk Access → \
                     enable your terminal app (Terminal, iTerm2, etc.)\n  \
                     Falling back to local database until then.",
                    path = new_path.display(),
                );
                Self::open_and_migrate_at(old_path.clone())?
            }
            Err(err) => return Err(err),
        };

        // Auto-merge: on first run after upgrade, if the old local DB exists at a
        // different path (e.g. we just moved to iCloud), merge it in and archive it.
        // The rename to .bak makes this idempotent — subsequent launches skip it.
        let effective_path = &storage.db_path;
        if old_path != *effective_path && old_path.exists() {
            let computer = computer_name();
            if let Err(err) = merge_old_db_into(effective_path, &old_path, &computer) {
                eprintln!("looper: warning — could not merge old history: {err}");
            } else {
                let bak = old_path.with_extension("sqlite3.bak");
                if let Err(err) = fs::rename(&old_path, &bak) {
                    eprintln!("looper: warning — could not archive old DB: {err}");
                }
            }
        }

        Ok(storage)
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
        }),
        PlaybackInput::ProcessStdout { .. } => Err(eyre!(
            "cannot derive persistent track identity without a source URL"
        )),
    }
}

/// Resolves where the DB should live, in priority order:
/// 1. User-configured sync folder (`~/.config/looper/sync_folder` plain-text file)
/// 2. iCloud Drive on macOS (`~/Library/Mobile Documents/com~apple~CloudDocs/looper/`)
/// 3. Platform data directory (existing default behavior)
pub fn resolve_db_path() -> Result<PathBuf> {
    if let Some(folder) = read_sync_folder_config() {
        fs::create_dir_all(&folder).wrap_err("failed to create configured sync folder")?;
        return Ok(folder.join("looper.sqlite3"));
    }

    #[cfg(target_os = "macos")]
    if let Some(path) = icloud_db_path() {
        return Ok(path);
    }

    default_db_path()
}

#[cfg(target_os = "macos")]
fn icloud_db_path() -> Option<PathBuf> {
    let home = directories::UserDirs::new()?.home_dir().to_path_buf();
    let icloud_root = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if !icloud_root.exists() {
        return None;
    }
    let looper_dir = icloud_root.join("looper");
    fs::create_dir_all(&looper_dir).ok()?;
    Some(looper_dir.join("looper.sqlite3"))
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

/// Merges all rows from `old_path` into the DB at `new_path`.
///
/// Merge semantics per track:
///   play_count         → summed
///   total_play_seconds → summed
///   first_played_at    → earliest of both
///   last_played_at     → latest of both
///   is_favorite        → true if either copy is true
///   last_played_computer → from whichever had the more recent last_played_at
///
/// Runs pending migrations on the old DB first so missing columns (e.g.
/// `last_played_computer`) are added before reading.
fn merge_old_db_into(new_path: &Path, old_path: &Path, computer: &str) -> Result<()> {
    use crate::schema::played_tracks::dsl as tracks;

    let mut old_conn = establish_connection(old_path)?;
    old_conn
        .run_pending_migrations(MIGRATIONS)
        .map_err(|e| eyre!("failed to migrate old DB before merge: {e}"))?;

    let old_rows: Vec<PlayedTrackRow> = tracks::played_tracks
        .select(PlayedTrackRow::as_select())
        .load(&mut old_conn)?;

    let mut new_conn = establish_connection(new_path)?;

    for row in old_rows {
        let row_computer = if row.last_played_computer.is_empty() {
            computer.to_string()
        } else {
            row.last_played_computer.clone()
        };

        let existing = tracks::played_tracks
            .filter(tracks::track_key.eq(&row.track_key))
            .select(PlayedTrackRow::as_select())
            .first::<PlayedTrackRow>(&mut new_conn)
            .optional()
            .map_err(|e| eyre!("merge read error: {e}"))?;

        if let Some(existing) = existing {
            let merged_computer = if row.last_played_at > existing.last_played_at {
                row_computer.clone()
            } else {
                existing.last_played_computer.clone()
            };
            diesel::update(
                tracks::played_tracks.filter(tracks::track_key.eq(&row.track_key)),
            )
            .set((
                tracks::play_count.eq(existing.play_count + row.play_count),
                tracks::total_play_seconds
                    .eq(existing.total_play_seconds + row.total_play_seconds),
                tracks::first_played_at
                    .eq(existing.first_played_at.min(row.first_played_at)),
                tracks::last_played_at
                    .eq(existing.last_played_at.max(row.last_played_at)),
                tracks::is_favorite.eq(existing.is_favorite || row.is_favorite),
                tracks::last_played_computer.eq(merged_computer),
            ))
            .execute(&mut new_conn)
            .map_err(|e| eyre!("merge update error: {e}"))?;
        } else {
            let new_row = NewPlayedTrack {
                track_key: &row.track_key,
                replay_target: &row.replay_target,
                title: &row.title,
                platform: &row.platform,
                is_favorite: row.is_favorite,
                play_count: row.play_count,
                total_play_seconds: row.total_play_seconds,
                first_played_at: row.first_played_at,
                last_played_at: row.last_played_at,
                last_played_computer: &row_computer,
            };
            diesel::insert_into(tracks::played_tracks)
                .values(&new_row)
                .execute(&mut new_conn)
                .map_err(|e| eyre!("merge insert error: {e}"))?;
        }
    }

    Ok(())
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
        };
        storage.record_play(&record).unwrap();
        let rows = storage
            .list_history(HistorySortField::LastPlayed, false)
            .unwrap();
        assert!(!rows[0].last_played_computer.is_empty());
    }

    #[test]
    fn merge_combines_play_counts() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let path_a = dir_a.path().join("a.sqlite3");
        let path_b = dir_b.path().join("b.sqlite3");

        let storage_a = Storage::open_and_migrate_at(path_a.clone()).unwrap();
        let record = TrackRecord {
            track_key: "key-x".into(),
            replay_target: "key-x".into(),
            title: "X".into(),
            platform: "Local".into(),
        };
        storage_a.record_play(&record).unwrap();
        storage_a.record_play(&record).unwrap();
        storage_a.record_play(&record).unwrap();
        storage_a.record_playback_time("key-x", 30).unwrap();

        let storage_b = Storage::open_and_migrate_at(path_b.clone()).unwrap();
        storage_b.record_play(&record).unwrap();
        storage_b.record_play(&record).unwrap();
        storage_b.record_playback_time("key-x", 20).unwrap();
        let record_y = TrackRecord {
            track_key: "key-y".into(),
            replay_target: "key-y".into(),
            title: "Y".into(),
            platform: "Local".into(),
        };
        storage_b.record_play(&record_y).unwrap();
        drop(storage_a);
        drop(storage_b);

        merge_old_db_into(&path_a, &path_b, "Computer B").unwrap();

        let storage_a = Storage::open_and_migrate_at(path_a).unwrap();
        let rows = storage_a
            .list_history(HistorySortField::PlayCount, true)
            .unwrap();

        let x = rows.iter().find(|r| r.track_key == "key-x").unwrap();
        assert_eq!(x.play_count, 5, "play_count should be summed (3+2)");
        assert_eq!(x.total_play_seconds, 50, "total_play_seconds should be summed (30+20)");

        let y = rows.iter().find(|r| r.track_key == "key-y").unwrap();
        assert_eq!(y.play_count, 1);
        // last_played_computer is set by record_play at insert time — just verify it's non-empty
        assert!(!y.last_played_computer.is_empty());
    }

    #[test]
    fn merge_preserves_favorite() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let path_a = dir_a.path().join("a.sqlite3");
        let path_b = dir_b.path().join("b.sqlite3");

        let storage_a = Storage::open_and_migrate_at(path_a.clone()).unwrap();
        let storage_b = Storage::open_and_migrate_at(path_b.clone()).unwrap();
        let record = TrackRecord {
            track_key: "track-1".into(),
            replay_target: "track-1".into(),
            title: "One".into(),
            platform: "Local".into(),
        };
        storage_a.record_play(&record).unwrap();
        storage_a.toggle_favorite("track-1").unwrap();
        storage_b.record_play(&record).unwrap();
        drop(storage_a);
        drop(storage_b);

        merge_old_db_into(&path_a, &path_b, "Computer B").unwrap();

        let storage_a = Storage::open_and_migrate_at(path_a).unwrap();
        assert!(
            storage_a.favorite_for("track-1").unwrap(),
            "favorite should be preserved (OR semantics)"
        );
    }

    #[test]
    fn records_first_play() {
        let (_dir, storage) = test_storage();
        let record = TrackRecord {
            track_key: "https://example.com/a".into(),
            replay_target: "https://example.com/a".into(),
            title: "A".into(),
            platform: "YouTube".into(),
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
            })
            .unwrap();
        storage.record_playback_time("b", 15).unwrap();
        storage
            .record_play(&TrackRecord {
                track_key: "a".into(),
                replay_target: "a".into(),
                title: "Alpha".into(),
                platform: "SoundCloud".into(),
            })
            .unwrap();
        storage.record_playback_time("a", 60).unwrap();

        let rows = storage
            .list_history(HistorySortField::TimePlayed, true)
            .unwrap();
        assert_eq!(rows[0].title, "Alpha");
        assert_eq!(rows[1].title, "Beta");
    }
}
