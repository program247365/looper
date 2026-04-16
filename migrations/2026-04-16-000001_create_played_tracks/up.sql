CREATE TABLE played_tracks (
    track_key TEXT PRIMARY KEY NOT NULL,
    replay_target TEXT NOT NULL,
    title TEXT NOT NULL,
    platform TEXT NOT NULL,
    is_favorite BOOLEAN NOT NULL DEFAULT 0,
    play_count BIGINT NOT NULL,
    first_played_at BIGINT NOT NULL,
    last_played_at BIGINT NOT NULL
);
