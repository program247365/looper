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
    LastPlayed,
    Platform,
    Title,
    PlayCount,
}

impl HistorySortField {
    pub fn label(self) -> &'static str {
        match self {
            Self::LastPlayed => "Last Played",
            Self::Platform => "Platform",
            Self::Title => "Title",
            Self::PlayCount => "Times Played",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::LastPlayed => Self::Platform,
            Self::Platform => Self::Title,
            Self::Title => Self::PlayCount,
            Self::PlayCount => Self::LastPlayed,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::LastPlayed => Self::PlayCount,
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
    pub first_played_at: i64,
    pub last_played_at: i64,
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
    first_played_at: i64,
    last_played_at: i64,
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
    first_played_at: i64,
    last_played_at: i64,
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
            first_played_at: row.first_played_at,
            last_played_at: row.last_played_at,
        }
    }
}

impl Storage {
    pub fn open_and_migrate() -> Result<Self> {
        let db_path = default_db_path()?;
        Self::open_and_migrate_at(db_path)
    }

    pub fn shared(self) -> SharedStorage {
        Arc::new(Mutex::new(self))
    }

    pub fn open_and_migrate_at(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).wrap_err("failed to create looper data directory")?;
        }

        let mut connection = establish_connection(&db_path)?;
        connection
            .run_pending_migrations(MIGRATIONS)
            .map_err(|err| eyre!("failed to run looper database migrations: {err}"))?;

        Ok(Self { db_path })
    }

    pub fn record_play(&self, record: &TrackRecord) -> Result<()> {
        use crate::schema::played_tracks::dsl as tracks;

        let now = unix_timestamp();
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
                    first_played_at: now,
                    last_played_at: now,
                };
                diesel::insert_into(tracks::played_tracks)
                    .values(&row)
                    .execute(conn)?;
            }

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
        storage
            .record_play(&TrackRecord {
                track_key: "a".into(),
                replay_target: "a".into(),
                title: "Alpha".into(),
                platform: "SoundCloud".into(),
            })
            .unwrap();

        let rows = storage
            .list_history(HistorySortField::Title, false)
            .unwrap();
        assert_eq!(rows[0].title, "Alpha");
        assert_eq!(rows[1].title, "Beta");
    }
}
