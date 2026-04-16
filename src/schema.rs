diesel::table! {
    played_tracks (track_key) {
        track_key -> Text,
        replay_target -> Text,
        title -> Text,
        platform -> Text,
        is_favorite -> Bool,
        play_count -> BigInt,
        total_play_seconds -> BigInt,
        first_played_at -> BigInt,
        last_played_at -> BigInt,
    }
}
