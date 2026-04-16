CREATE TABLE played_tracks_new (
    track_key TEXT PRIMARY KEY NOT NULL,
    replay_target TEXT NOT NULL,
    title TEXT NOT NULL,
    platform TEXT NOT NULL,
    is_favorite BOOLEAN NOT NULL DEFAULT 0,
    play_count BIGINT NOT NULL,
    first_played_at BIGINT NOT NULL,
    last_played_at BIGINT NOT NULL
);

INSERT INTO played_tracks_new (
    track_key,
    replay_target,
    title,
    platform,
    is_favorite,
    play_count,
    first_played_at,
    last_played_at
)
SELECT
    track_key,
    replay_target,
    title,
    platform,
    is_favorite,
    play_count,
    first_played_at,
    last_played_at
FROM played_tracks;

DROP TABLE played_tracks;
ALTER TABLE played_tracks_new RENAME TO played_tracks;
