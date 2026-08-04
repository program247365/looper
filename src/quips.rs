//! The startup screen's one-liner pool. Add or remove sayings freely — the
//! picker stays in bounds because it derives from `QUIPS.len()`.

use std::sync::OnceLock;

const QUIPS: &[&str] = &[
    "warming up the loop engine",
    "convincing sqlite this is definitely a music venue",
    "dusting fingerprints off the play count ledger",
    "aligning vibes, bits, and questionable dance moves",
    "blowing the dust off the needle",
    "untangling the aux cord from last night",
    "rewinding the tape with a pencil",
    "flipping the record to side B",
    "skipping back to the good part",
    "calibrating the vibe to within one decimal",
    "checking the vibes are still where you left them",
    "turning it up to eleven — it's one louder, isn't it",
    "restoring vibes from the last known good loop",
    "leaving the volume knob exactly where you like it",
];

/// One quip per launch: the startup screens rebuild their state every frame,
/// so re-rolling here would flicker a new joke 30 times a second.
pub fn startup_quip() -> &'static str {
    static QUIP: OnceLock<&'static str> = OnceLock::new();
    QUIP.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize;
        QUIPS[nanos % QUIPS.len()]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quip_is_from_the_pool() {
        assert!(QUIPS.contains(&startup_quip()));
    }

    #[test]
    fn quip_is_stable_within_a_run() {
        assert_eq!(startup_quip(), startup_quip());
    }
}
