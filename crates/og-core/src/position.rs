//! Board representation.

/// A backgammon position, always seen from the perspective of the player on roll.
///
/// # Indexing
///
/// `points[i]` holds point `i + 1`, so index range `0..=23` covers points `1..=24`.
/// The numbering is **relative to the player on roll**, not absolute White/Black:
///
/// - The player on roll's home board is points `1..=6` (indices `0..=5`).
/// - The player on roll's checkers move in the direction of decreasing point number,
///   i.e. from index 23 toward index 0, and bear off past point 1 (past index 0).
/// - The opponent's home board is therefore points `19..=24` (indices `18..=23`) in
///   this same relative frame — that is where the player on roll's checkers enter
///   from the bar, and where the opponent bears off *from* (in the opponent's own,
///   mirrored frame).
///
/// A checker entering from the bar with die value `d` lands on point `25 - d`,
/// i.e. index `24 - d` (die 1 enters deepest, on point 24; die 6 enters shallowest,
/// on point 19).
///
/// # Sign
///
/// `points[i]` is a signed checker count:
///
/// - Positive: that many of the player-on-roll's checkers occupy the point.
/// - Negative: that many of the opponent's checkers occupy the point (e.g. `-3`
///   means 3 opposing checkers, not 3 checkers belonging to the player on roll).
/// - Zero: empty.
///
/// A point is never simultaneously occupied by both sides.
///
/// # No color
///
/// `Position` does not know or store which side is White/Black — it only knows
/// "me" (player on roll, positive) and "them" (opponent, negative). To generate
/// the opponent's replies, the board is mirrored: point `p` maps to point `25 - p`
/// (index `i` maps to index `23 - i`) and signs are negated. There is no other
/// place in this crate where point numbering is allowed to mean anything else —
/// any function that receives or returns a point/index must use this convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub(crate) points: [i8; 24],
    /// Checkers on the bar: `[mine, opponent's]`.
    pub(crate) bar: [u8; 2],
    /// Checkers already borne off: `[mine, opponent's]`.
    pub(crate) off: [u8; 2],
}

impl Position {
    /// The standard starting position, from the perspective of the player on roll.
    ///
    /// Mine: 2 on point 24, 5 on point 13, 3 on point 8, 5 on point 6.
    /// Opponent's (mirrored into this frame): 2 on point 1, 5 on point 12,
    /// 3 on point 17, 5 on point 19.
    pub fn starting() -> Self {
        let mut points = [0i8; 24];
        points[23] = 2; // point 24: mine
        points[12] = 5; // point 13: mine
        points[7] = 3; // point 8: mine
        points[5] = 5; // point 6: mine
        points[0] = -2; // point 1: opponent's
        points[11] = -5; // point 12: opponent's
        points[16] = -3; // point 17: opponent's
        points[18] = -5; // point 19: opponent's

        Position {
            points,
            bar: [0, 0],
            off: [0, 0],
        }
    }

    /// Checker count at `points[index]` (index `0..=23` = points `1..=24`).
    /// Positive = player on roll, negative = opponent, per the type-level doc.
    pub fn point(&self, index: usize) -> i8 {
        self.points[index]
    }

    /// Checkers on the bar: `[mine, opponent's]`.
    pub fn bar(&self) -> [u8; 2] {
        self.bar
    }

    /// Checkers borne off: `[mine, opponent's]`.
    pub fn off(&self) -> [u8; 2] {
        self.off
    }

    /// Builds a position directly from raw fields, bypassing `starting()`.
    ///
    /// Test-only: does not validate checker-count invariants (e.g. 15 per side).
    /// Move generation tests use this to set up minimal, targeted board states.
    #[cfg(test)]
    pub(crate) fn from_raw(points: [i8; 24], bar: [u8; 2], off: [u8; 2]) -> Self {
        Position { points, bar, off }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_position_is_standard() {
        let pos = Position::starting();

        // Mine.
        assert_eq!(pos.point(23), 2, "point 24");
        assert_eq!(pos.point(12), 5, "point 13");
        assert_eq!(pos.point(7), 3, "point 8");
        assert_eq!(pos.point(5), 5, "point 6");

        // Opponent's.
        assert_eq!(pos.point(0), -2, "point 1");
        assert_eq!(pos.point(11), -5, "point 12");
        assert_eq!(pos.point(16), -3, "point 17");
        assert_eq!(pos.point(18), -5, "point 19");

        assert_eq!(pos.bar(), [0, 0]);
        assert_eq!(pos.off(), [0, 0]);

        // Every checker accounted for, no point double-occupied.
        let mine: i32 = pos
            .points
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| c as i32)
            .sum();
        let theirs: i32 = pos
            .points
            .iter()
            .filter(|&&c| c < 0)
            .map(|&c| -c as i32)
            .sum();
        assert_eq!(mine, 15);
        assert_eq!(theirs, 15);
    }
}
