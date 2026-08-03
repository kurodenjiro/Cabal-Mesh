//! The reputation score — currently mocked, deliberately and in one place.
//!
//! # The decision this module records (ticket 03)
//!
//! The prototype showed `REPUTATION SCORE 87.6 (+5.3%)` on home and profile as
//! a constant. No signal behind it exists anywhere in the codebase, and until
//! now both screens rendered an em dash rather than invent one.
//!
//! The call was made to ship a **mock value** so the screens read as designed
//! before a real signal exists. That is worth naming plainly rather than
//! burying: the brand's copy rules treat exact figures as a trust signal, and a
//! derived-from-nothing number is a claim the system cannot back. The
//! mitigations are that it lives here and nowhere else, that it is derived
//! rather than random, and that ticket 39 exists to replace it.
//!
//! # Why derived from the node identifier rather than random
//!
//! Home polls every five seconds and profile does the same. A score sampled per
//! call would visibly jitter — 87.6, then 64.2, then 91.0 — which does not read
//! as a mock, it reads as a bug. Deriving from the peer identifier makes the
//! value stable for the lifetime of an identity and different between nodes,
//! which is what a demo across two devices needs.
//!
//! # Why offline still renders an em dash
//!
//! With no mesh there is no peer identifier, so there is nothing to derive
//! from. Falling back to a fixed number there would put the same score on every
//! device in every screenshot. [`Reputation::of`] returns `None` and both
//! screens keep the honest placeholder they already had.

/// A reputation reading: the score and its period-over-period delta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reputation {
    /// `60.0` to `99.9`.
    pub score: f64,
    /// `-9.9` to `+9.9`, as a percentage.
    pub delta_percent: f64,
}

impl Reputation {
    /// The mock reading for a node, or `None` when there is no node to derive
    /// from.
    ///
    /// An empty or placeholder identifier means the mesh is not up yet, which
    /// is a genuinely unmeasured state rather than one to fill in.
    #[must_use]
    pub fn of(peer_id: &str) -> Option<Self> {
        if peer_id.is_empty() || peer_id == "—" {
            return None;
        }

        let hash = fnv1a(peer_id.as_bytes());

        // Split the hash rather than hashing twice: the halves of a 64-bit
        // FNV-1a are independent enough for a value nobody is meant to trust.
        let score = 60.0 + f64::from((hash % 400) as u16) / 10.0;
        let delta_percent = f64::from(((hash >> 32) % 199) as u16) / 10.0 - 9.9;

        Some(Self { score, delta_percent })
    }

    /// `87.6` — the score alone, for the home tile that carries its delta in a
    /// separate field.
    #[must_use]
    pub fn value(&self) -> String {
        format!("{:.1}", self.score)
    }

    /// `87.6 (+5.3%)` — score and delta in one string, the shape the profile
    /// row renders.
    #[must_use]
    pub fn combined(&self) -> String {
        format!("{:.1} ({:+.1}%)", self.score, self.delta_percent)
    }
}

/// FNV-1a, 64-bit.
///
/// Not for security and not for distribution quality — it is here because it is
/// four lines and needs no dependency, and the only requirement on it is that
/// two peer identifiers rarely land on the same score.
const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
        index += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_node_always_reads_the_same() {
        // The property that makes this a mock rather than a bug: home and
        // profile poll every five seconds, and a value that moved between polls
        // would look like the score was broken.
        let first = Reputation::of("12D3KooWExample").unwrap();
        let second = Reputation::of("12D3KooWExample").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn different_nodes_read_differently() {
        let a = Reputation::of("12D3KooWAlpha").unwrap();
        let b = Reputation::of("12D3KooWBravo").unwrap();
        assert_ne!(a.score, b.score);
    }

    #[test]
    fn no_node_means_no_score() {
        // Not a fallback constant: every device without a mesh would otherwise
        // show the same number in every screenshot.
        assert!(Reputation::of("").is_none());
        assert!(Reputation::of("—").is_none());
    }

    #[test]
    fn readings_stay_inside_their_bands() {
        // Sampled rather than exhaustive, but wide enough to catch an off-by-one
        // in either range expression.
        for index in 0..2_000 {
            let reading = Reputation::of(&format!("12D3KooW{index}")).unwrap();
            assert!(
                (60.0..=99.9).contains(&reading.score),
                "score out of band: {}",
                reading.score
            );
            assert!(
                (-9.9..=9.9).contains(&reading.delta_percent),
                "delta out of band: {}",
                reading.delta_percent
            );
        }
    }

    #[test]
    fn formatting_matches_the_two_places_it_renders() {
        let reading = Reputation { score: 87.6, delta_percent: 5.3 };
        assert_eq!(reading.value(), "87.6");
        assert_eq!(reading.combined(), "87.6 (+5.3%)");

        // The sign is always explicit — a bare "3.0%" leaves the direction to
        // the colour, which reaches no screen reader.
        let falling = Reputation { score: 64.0, delta_percent: -3.0 };
        assert_eq!(falling.combined(), "64.0 (-3.0%)");
    }
}
