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

/// A raw field combination passed to [`Position::from_raw`] that cannot represent any
/// real or partial backgammon state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    /// One side's checkers (points, bar, and off combined) sum to more than 15.
    /// `mine` says which side; `count` is the sum that was too high.
    TooManyCheckers { mine: bool, count: u32 },
}

impl std::fmt::Display for PositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionError::TooManyCheckers { mine: true, count } => {
                write!(
                    f,
                    "player on roll has {count} checkers, more than the 15 allowed"
                )
            }
            PositionError::TooManyCheckers { mine: false, count } => {
                write!(f, "opponent has {count} checkers, more than the 15 allowed")
            }
        }
    }
}

impl std::error::Error for PositionError {}

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
    /// Validates that each side has at most 15 checkers across points, bar, and off
    /// combined. Deliberately accepts *partial* positions — e.g. only one side's
    /// checkers set, the rest zero — not just complete, legal two-sided boards: a
    /// one-sided bearoff computation (`og-bearoff`) only knows about one side's
    /// checkers and has no need to invent a real opponent to satisfy this constructor.
    /// It does not check finer invariants such as reachability from the starting
    /// position, since "at most 15 checkers" is the only invariant every caller
    /// actually depends on.
    ///
    /// # Errors
    /// Returns [`PositionError`] if either side's checkers (positive `points` values,
    /// or negative ones by absolute value, plus that side's `bar` and `off` slot) sum
    /// to more than 15.
    pub fn from_raw(points: [i8; 24], bar: [u8; 2], off: [u8; 2]) -> Result<Self, PositionError> {
        let mine: u32 = points
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| c as u32)
            .sum::<u32>()
            + bar[0] as u32
            + off[0] as u32;
        if mine > 15 {
            return Err(PositionError::TooManyCheckers {
                mine: true,
                count: mine,
            });
        }

        let theirs: u32 = points
            .iter()
            .filter(|&&c| c < 0)
            .map(|&c| (-c) as u32)
            .sum::<u32>()
            + bar[1] as u32
            + off[1] as u32;
        if theirs > 15 {
            return Err(PositionError::TooManyCheckers {
                mine: false,
                count: theirs,
            });
        }

        Ok(Position { points, bar, off })
    }

    /// Builds a position directly from raw fields, without validating the
    /// at-most-15-checkers-per-side invariant that [`from_raw`](Self::from_raw) checks.
    ///
    /// Crate-internal only: for hot paths that only ever transform an
    /// already-known-valid `Position` (where re-validating on every call would be
    /// pure overhead), and for tests that need minimal, targeted board states without
    /// paying attention to the invariant at all. `#[cfg(test)]` for now since no
    /// non-test caller exists yet — drop the gate when one does.
    #[cfg(test)]
    pub(crate) fn from_raw_unchecked(points: [i8; 24], bar: [u8; 2], off: [u8; 2]) -> Self {
        Position { points, bar, off }
    }

    /// Switches perspective: the position seen by the player who was "them"
    /// in `self` becomes "me" here, per the convention documented on this
    /// type (point `p` maps to point `25 - p`, signs negate, `bar`/`off`
    /// swap slots). Applying a ply and then mirroring is how a turn passes
    /// from one player to the other.
    ///
    /// An involution: `p.mirror().mirror() == p`.
    pub fn mirror(&self) -> Position {
        let mut points = [0i8; 24];
        for (i, &count) in self.points.iter().enumerate() {
            points[23 - i] = -count;
        }
        Position {
            points,
            bar: [self.bar[1], self.bar[0]],
            off: [self.off[1], self.off[0]],
        }
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

    #[test]
    fn from_raw_accepts_a_full_legal_position() {
        let mut points = [0i8; 24];
        points[23] = 2;
        points[0] = -2;
        assert!(Position::from_raw(points, [0, 0], [13, 13]).is_ok());
    }

    #[test]
    fn from_raw_accepts_a_one_sided_partial_position() {
        // Only "mine" checkers set, opponent entirely absent (all zero). This is
        // exactly the shape `og-bearoff`'s one-sided database needs: it only knows
        // about one side's checkers and has no real opponent to place anywhere.
        let mut points = [0i8; 24];
        points[5] = 15; // all 15 on point 6, nothing else on the board
        assert!(Position::from_raw(points, [0, 0], [0, 0]).is_ok());
    }

    #[test]
    fn from_raw_accepts_exactly_15_on_one_side() {
        let mut points = [0i8; 24];
        points[5] = 10;
        assert!(Position::from_raw(points, [2, 0], [3, 0]).is_ok());
    }

    #[test]
    fn from_raw_rejects_too_many_mine() {
        let mut points = [0i8; 24];
        points[5] = 10;
        let err = Position::from_raw(points, [3, 0], [3, 0]).unwrap_err();
        assert_eq!(
            err,
            PositionError::TooManyCheckers {
                mine: true,
                count: 16
            }
        );
    }

    #[test]
    fn from_raw_rejects_too_many_theirs() {
        let mut points = [0i8; 24];
        points[18] = -10;
        let err = Position::from_raw(points, [0, 3], [0, 3]).unwrap_err();
        assert_eq!(
            err,
            PositionError::TooManyCheckers {
                mine: false,
                count: 16
            }
        );
    }

    #[test]
    fn mirror_of_starting_position_is_itself() {
        // The starting position is symmetric: what's "theirs" at point p is
        // exactly what's "mine" at point 25-p, so mirroring changes nothing.
        assert_eq!(Position::starting().mirror(), Position::starting());
    }

    #[test]
    fn mirror_flips_points_bar_and_off() {
        let mut points = [0i8; 24];
        points[0] = 2; // mine, point 1
        points[23] = -3; // theirs, point 24
        let position = Position::from_raw_unchecked(points, [1, 2], [4, 5]);

        let mirrored = position.mirror();

        assert_eq!(
            mirrored.point(23),
            -2,
            "my point 1 checkers are now theirs at point 24"
        );
        assert_eq!(
            mirrored.point(0),
            3,
            "their point 24 checkers are now mine at point 1"
        );
        assert_eq!(mirrored.bar(), [2, 1], "bar slots swap");
        assert_eq!(mirrored.off(), [5, 4], "off slots swap");
    }

    #[test]
    fn mirror_is_its_own_inverse() {
        let mut points = [0i8; 24];
        points[4] = 1;
        points[9] = -2;
        let position = Position::from_raw_unchecked(points, [1, 0], [3, 2]);

        assert_eq!(position.mirror().mirror(), position);
    }
}
