//! Dice, single-checker moves, full plies, and legal move generation.

use std::collections::{BTreeSet, HashMap};

use crate::position::Position;

/// An index into `Position`'s points, `0..=23` (points `1..=24`). See the
/// [`Position`] doc comment for the indexing and sign convention.
///
/// A distinct type from a bare `u8` so the compiler rejects mixing a point
/// index with the `Bar`/`Off` sentinels a raw-`u8` encoding would need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointIndex(u8);

impl PointIndex {
    /// Wraps `index` as a point index.
    ///
    /// Panics (debug builds only) if `index > 23` — every caller in this crate
    /// constructs indices from a `0..24` loop, so out-of-range values indicate
    /// a bug here, not bad external input.
    pub fn new(index: u8) -> Self {
        debug_assert!(index < 24, "point index out of range: {index}");
        PointIndex(index)
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// A single die value, `1..=6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Die(u8);

impl Die {
    /// Wraps `value` as a die value.
    ///
    /// Panics (debug builds only) if `value` is not in `1..=6`.
    pub fn new(value: u8) -> Self {
        debug_assert!((1..=6).contains(&value), "die value out of range: {value}");
        Die(value)
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// Where a checker moves from: the bar, or a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Bar,
    Point(PointIndex),
}

/// Where a checker moves to: a point, or off the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Point(PointIndex),
    Off,
}

/// A single checker's move within a [`Ply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckerMove {
    pub from: Origin,
    pub to: Destination,
}

/// A full turn for one roll: up to 4 [`CheckerMove`]s (more than 2 only for doubles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ply {
    moves: [Option<CheckerMove>; 4],
}

impl Ply {
    pub(crate) fn empty() -> Self {
        Ply { moves: [None; 4] }
    }

    /// Returns a copy of this ply with `checker_move` appended.
    ///
    /// Panics if already at 4 moves — provably impossible here, since a roll
    /// yields at most 4 dice and callers push at most once per die.
    pub(crate) fn pushed(mut self, checker_move: CheckerMove) -> Self {
        let slot = self
            .moves
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("a ply has at most 4 moves, bounded by dice count");
        *slot = Some(checker_move);
        self
    }

    pub fn len(&self) -> usize {
        self.moves.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn moves(&self) -> impl Iterator<Item = &CheckerMove> {
        self.moves.iter().filter_map(|slot| slot.as_ref())
    }
}

/// A dice roll. Dice are stored in canonical order (`dice().0 <= dice().1`),
/// so a roll and its mirror (e.g. `3-5` and `5-3`) compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roll {
    dice: (Die, Die),
}

impl Roll {
    pub fn new(a: Die, b: Die) -> Self {
        if a.get() <= b.get() {
            Roll { dice: (a, b) }
        } else {
            Roll { dice: (b, a) }
        }
    }

    pub fn is_double(&self) -> bool {
        self.dice.0 == self.dice.1
    }

    pub fn dice(&self) -> (Die, Die) {
        self.dice
    }

    /// The dice as they're consumed during move generation: 4 copies for a
    /// double, the 2 distinct values otherwise.
    fn as_multiset(&self) -> Vec<Die> {
        if self.is_double() {
            vec![self.dice.0; 4]
        } else {
            vec![self.dice.0, self.dice.1]
        }
    }
}

fn remove_one(dice: &[Die], value: u8) -> Vec<Die> {
    let mut result = Vec::with_capacity(dice.len().saturating_sub(1));
    let mut removed = false;
    for &die in dice {
        if !removed && die.get() == value {
            removed = true;
            continue;
        }
        result.push(die);
    }
    result
}

impl Position {
    /// Applies `ply` to this position, returning the resulting position.
    ///
    /// `ply` is assumed to be legal for `self` (as produced by
    /// [`Position::generate_moves`]); this method does not re-validate
    /// legality.
    pub fn apply(&self, ply: &Ply) -> Position {
        let mut result = *self;
        for checker_move in ply.moves() {
            result.apply_one(*checker_move);
        }
        result
    }

    fn apply_one(&mut self, checker_move: CheckerMove) {
        match checker_move.from {
            Origin::Bar => self.bar[0] -= 1,
            Origin::Point(p) => self.points[p.get() as usize] -= 1,
        }
        match checker_move.to {
            Destination::Off => self.off[0] += 1,
            Destination::Point(p) => {
                let idx = p.get() as usize;
                if self.points[idx] == -1 {
                    // A lone opposing checker is hit: sent to the opponent's bar.
                    self.points[idx] = 0;
                    self.bar[1] += 1;
                }
                self.points[idx] += 1;
            }
        }
    }

    fn all_checkers_home(&self) -> bool {
        self.bar[0] == 0 && self.points[6..24].iter().all(|&count| count <= 0)
    }

    /// Legal single-checker moves for `die`, ignoring the rest of the roll.
    fn legal_origins_for_die(&self, die: u8) -> Vec<CheckerMove> {
        let mut result = Vec::new();

        if self.bar[0] > 0 {
            let entry_index = 24 - die;
            if self.points[entry_index as usize] > -2 {
                result.push(CheckerMove {
                    from: Origin::Bar,
                    to: Destination::Point(PointIndex::new(entry_index)),
                });
            }
            return result;
        }

        let home = self.all_checkers_home();

        for (i, &count) in self.points.iter().enumerate() {
            if count <= 0 {
                continue;
            }

            let target = i as i32 - die as i32;
            if target >= 0 {
                let target_idx = target as usize;
                if self.points[target_idx] > -2 {
                    result.push(CheckerMove {
                        from: Origin::Point(PointIndex::new(i as u8)),
                        to: Destination::Point(PointIndex::new(target_idx as u8)),
                    });
                }
            } else if home {
                let point_number = i + 1;
                let exact = point_number == die as usize;
                let overage = die as usize > point_number
                    && !self.points[i + 1..6].iter().any(|&higher| higher > 0);
                if exact || overage {
                    result.push(CheckerMove {
                        from: Origin::Point(PointIndex::new(i as u8)),
                        to: Destination::Off,
                    });
                }
            }
        }

        result
    }

    /// Depth-first search over die-value choices, recording every terminal
    /// (no-more-legal-moves) state reached, however many dice it used.
    fn collect_plies(
        &self,
        dice_remaining: &[Die],
        ply_so_far: Ply,
        dice_used: Vec<u8>,
        leaves: &mut Vec<(Position, Ply, Vec<u8>)>,
    ) {
        let distinct: BTreeSet<u8> = dice_remaining.iter().map(|d| d.get()).collect();
        let mut extended = false;

        for value in distinct {
            for checker_move in self.legal_origins_for_die(value) {
                extended = true;

                let mut next_position = *self;
                next_position.apply_one(checker_move);

                let next_ply = ply_so_far.pushed(checker_move);
                let mut next_dice_used = dice_used.clone();
                next_dice_used.push(value);
                let next_dice_remaining = remove_one(dice_remaining, value);

                next_position.collect_plies(&next_dice_remaining, next_ply, next_dice_used, leaves);
            }
        }

        if !extended {
            leaves.push((*self, ply_so_far, dice_used));
        }
    }

    /// Generates all legal plies for `roll`, one per distinct resulting
    /// position (as GNUbg does — otherwise sub-move orderings that reach the
    /// same final position would be counted as distinct plays).
    ///
    /// Enforces, in order: use as many dice as legally possible; for a
    /// non-double where only one die can be played at all, prefer the higher
    /// die if both are individually playable.
    pub fn generate_moves(&self, roll: Roll) -> Vec<Ply> {
        let dice = roll.as_multiset();
        let mut leaves = Vec::new();
        self.collect_plies(&dice, Ply::empty(), Vec::new(), &mut leaves);

        let max_len = leaves
            .iter()
            .map(|(_, ply, _)| ply.len())
            .max()
            .unwrap_or(0);
        if max_len == 0 {
            return Vec::new();
        }

        let mut filtered: Vec<_> = leaves
            .into_iter()
            .filter(|(_, ply, _)| ply.len() == max_len)
            .collect();

        if !roll.is_double() && max_len == 1 {
            let higher = roll.dice().1.get();
            if filtered
                .iter()
                .any(|(_, _, dice_used)| dice_used[0] == higher)
            {
                filtered.retain(|(_, _, dice_used)| dice_used[0] == higher);
            }
        }

        let mut by_position: HashMap<Position, Ply> = HashMap::new();
        for (position, ply, _) in filtered {
            by_position.entry(position).or_insert(ply);
        }
        by_position.into_values().collect()
    }
}

#[cfg(test)]
mod apply_tests {
    use super::*;

    #[test]
    fn apply_moves_a_checker_between_points() {
        let position = Position::starting();
        let ply = Ply::empty().pushed(CheckerMove {
            from: Origin::Point(PointIndex::new(23)),
            to: Destination::Point(PointIndex::new(17)),
        });

        let result = position.apply(&ply);

        assert_eq!(result.point(23), 1);
        assert_eq!(result.point(17), 1);
        assert_eq!(result.bar(), [0, 0]);
    }

    #[test]
    fn apply_hits_a_blot_sending_it_to_the_bar() {
        let mut points = [0i8; 24];
        points[10] = 1; // mine
        points[5] = -1; // opponent's lone blot
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let ply = Ply::empty().pushed(CheckerMove {
            from: Origin::Point(PointIndex::new(10)),
            to: Destination::Point(PointIndex::new(5)),
        });
        let result = position.apply(&ply);

        assert_eq!(result.point(5), 1, "my checker now occupies the point");
        assert_eq!(
            result.bar(),
            [0, 1],
            "the hit blot goes to the opponent's bar"
        );
    }

    #[test]
    fn apply_enters_from_the_bar() {
        let position = Position::from_raw_unchecked([0i8; 24], [1, 0], [0, 0]);

        let ply = Ply::empty().pushed(CheckerMove {
            from: Origin::Bar,
            to: Destination::Point(PointIndex::new(21)),
        });
        let result = position.apply(&ply);

        assert_eq!(result.bar(), [0, 0]);
        assert_eq!(result.point(21), 1);
    }

    #[test]
    fn apply_bears_off() {
        let mut points = [0i8; 24];
        points[2] = 1;
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let ply = Ply::empty().pushed(CheckerMove {
            from: Origin::Point(PointIndex::new(2)),
            to: Destination::Off,
        });
        let result = position.apply(&ply);

        assert_eq!(result.point(2), 0);
        assert_eq!(result.off(), [1, 0]);
    }
}

#[cfg(test)]
mod generate_moves_tests {
    use super::*;

    /// Mirrors `collect_plies`'s exact recursion structure (same die-value
    /// loop, same `legal_origins_for_die`/`apply_one`/`remove_one`) but
    /// tracks recursion depth instead of collecting leaves — a faithful,
    /// zero-cost-in-production way to answer "how deep does the real
    /// algorithm actually recurse", not just "how deep could a DFS over N
    /// dice go in principle".
    fn collect_plies_depth(
        position: &Position,
        dice_remaining: &[Die],
        depth: u32,
        max_depth: &mut u32,
    ) {
        *max_depth = (*max_depth).max(depth);

        let distinct: BTreeSet<u8> = dice_remaining.iter().map(|d| d.get()).collect();
        for value in distinct {
            for checker_move in position.legal_origins_for_die(value) {
                let mut next_position = *position;
                next_position.apply_one(checker_move);
                let next_dice_remaining = remove_one(dice_remaining, value);
                collect_plies_depth(&next_position, &next_dice_remaining, depth + 1, max_depth);
            }
        }
    }

    #[test]
    fn recursion_depth_is_bounded_by_dice_count() {
        // collect_plies removes exactly one die per recursive call and never
        // adds one back, so depth is bounded by (dice count + 1) regardless
        // of branching factor: 5 for doubles, 3 otherwise. Branching factor
        // (how many legal moves exist at a given point) affects how much
        // *work* each level does and how many leaves accumulate, never how
        // many levels deep the call stack goes -- siblings in the `for` loop
        // run sequentially, not concurrently, so only one path is ever live
        // on the stack at a time.
        //
        // Verified here on dense, doubles-heavy positions (the worst case
        // for branching, per docs/rules-notes.md's MAX_MOVES finding) rather
        // than just asserted from reading the code.
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        use crate::self_play::random_reachable_position;

        let mut rng = StdRng::seed_from_u64(0xDEEC);
        let mut overall_max = 0u32;

        for turns in [0u32, 2, 4, 8, 15, 30, 60] {
            for _ in 0..50 {
                let position = random_reachable_position(&mut rng, turns);
                for d in 1..=6u8 {
                    let dice = vec![Die::new(d); 4]; // doubles: the deepest possible chain
                    let mut max_depth = 0;
                    collect_plies_depth(&position, &dice, 1, &mut max_depth);
                    overall_max = overall_max.max(max_depth);
                }
            }
        }

        eprintln!("max collect_plies recursion depth observed: {overall_max}");
        assert!(
            overall_max <= 5,
            "recursion depth {overall_max} exceeds the proven bound (dice count + 1 <= 5) -- the proof or the code has a bug"
        );
    }

    #[test]
    fn entry_from_bar() {
        // One checker on the bar; the point one step past its entry point is
        // blocked, so exactly one die is usable: the entry itself.
        let mut points = [0i8; 24];
        points[18] = -2; // blocks continuing past the entry point
        let position = Position::from_raw_unchecked(points, [1, 0], [0, 0]);

        let roll = Roll::new(Die::new(3), Die::new(3)); // double: no higher-die tiebreak to worry about
        let moves = position.generate_moves(roll);

        assert_eq!(moves.len(), 1);
        let ply = &moves[0];
        assert_eq!(ply.len(), 1);
        let checker_move = ply.moves().next().unwrap();
        assert_eq!(checker_move.from, Origin::Bar);
        assert_eq!(checker_move.to, Destination::Point(PointIndex::new(21)));

        let result = position.apply(ply);
        assert_eq!(result.bar()[0], 0);
        assert_eq!(result.point(21), 1);
    }

    #[test]
    fn entry_from_bar_blocked() {
        // One checker on the bar; every entry point is doubly covered, and an
        // otherwise-movable checker elsewhere must NOT move instead.
        let mut points = [0i8; 24];
        points[18..24].fill(-2);
        points[12] = 1; // would be movable if the bar didn't take precedence
        let position = Position::from_raw_unchecked(points, [1, 0], [0, 0]);

        let roll = Roll::new(Die::new(2), Die::new(5));
        assert!(position.generate_moves(roll).is_empty());
    }

    #[test]
    fn forced_higher_die_when_only_one_playable() {
        // A single checker can play the 3 alone or the 6 alone (landing point
        // for the other die is blocked either way), but never both. Must play
        // the 6.
        let mut points = [0i8; 24];
        points[12] = 1; // point 13
        points[3] = -2; // point 4, blocks the point both orderings need next
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let roll = Roll::new(Die::new(3), Die::new(6));
        let moves = position.generate_moves(roll);

        assert_eq!(moves.len(), 1);
        let ply = &moves[0];
        assert_eq!(ply.len(), 1);
        let checker_move = ply.moves().next().unwrap();
        assert_eq!(checker_move.from, Origin::Point(PointIndex::new(12)));
        assert_eq!(checker_move.to, Destination::Point(PointIndex::new(6)));
    }

    #[test]
    fn doubles_with_fewer_than_four_moves() {
        // Double 4s: a single checker can run two 4s (13 -> 9 -> 5) before the
        // third is blocked, so only 2 of the 4 dice are usable.
        let mut points = [0i8; 24];
        points[12] = 1; // point 13
        points[0] = -2; // point 1, blocks the third 4
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let roll = Roll::new(Die::new(4), Die::new(4));
        let moves = position.generate_moves(roll);

        assert_eq!(moves.len(), 1);
        let ply = &moves[0];
        assert_eq!(ply.len(), 2);

        let result = position.apply(ply);
        assert_eq!(result.point(12), 0);
        assert_eq!(result.point(4), 1);
    }

    #[test]
    fn bearoff_requires_all_checkers_home() {
        // One checker already home (point 6), one still outside (point 13).
        // Double 6s: bearing off point 6 immediately is illegal (not all
        // home), so the only legal sequence brings the outside checker home
        // first, then bears off both.
        let mut points = [0i8; 24];
        points[5] = 1; // point 6, home
        points[12] = 1; // point 13, not home
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let roll = Roll::new(Die::new(6), Die::new(6));
        let moves = position.generate_moves(roll);

        assert_eq!(moves.len(), 1);
        let ply = &moves[0];
        assert_eq!(ply.len(), 4, "all 4 sixes: 13->7->1, then both borne off");

        let result = position.apply(ply);
        assert_eq!(result.point(5), 0);
        assert_eq!(result.point(12), 0);
        assert_eq!(result.off()[0], 2);
    }

    #[test]
    fn bearoff_from_higher_die() {
        // Only checker at point 3, home. No checker on any higher home point,
        // so a 6 (bigger than the point number) can still bear it off.
        let mut points = [0i8; 24];
        points[2] = 1; // point 3
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let roll = Roll::new(Die::new(6), Die::new(6));
        let moves = position.generate_moves(roll);

        assert_eq!(moves.len(), 1);
        let ply = &moves[0];
        assert_eq!(
            ply.len(),
            1,
            "only 1 checker to bear off, the other 3 sixes are unusable"
        );
        let checker_move = ply.moves().next().unwrap();
        assert_eq!(checker_move.from, Origin::Point(PointIndex::new(2)));
        assert_eq!(checker_move.to, Destination::Off);
    }

    #[test]
    fn no_legal_moves() {
        // A single checker whose only two possible destinations (for either
        // die) are both blocked.
        let mut points = [0i8; 24];
        points[12] = 1; // point 13
        points[10] = -2; // blocks the 2
        points[9] = -2; // blocks the 3
        let position = Position::from_raw_unchecked(points, [0, 0], [0, 0]);

        let roll = Roll::new(Die::new(2), Die::new(3));
        assert!(position.generate_moves(roll).is_empty());
    }
}
