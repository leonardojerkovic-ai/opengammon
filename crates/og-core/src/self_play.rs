//! Random-but-*reachable* position generation for differential fuzz testing
//! against GNUbg (see `gnubg_diff`). Scattering checkers onto points at
//! random produces positions no real game could reach; playing random legal
//! games instead guarantees every position handed out is doigriva — see
//! CLAUDE.md §0.

use rand::seq::IndexedRandom;
use rand::{Rng, RngExt};

use crate::{Die, Position, Roll};

/// A single legal roll: each of the 36 die-value pairs is equally likely,
/// matching real dice. `Roll::new` canonicalizes order, so e.g. `3-5` and
/// `5-3` compare equal — each is reachable two ways out of 36, as it should
/// be.
pub(crate) fn random_roll(rng: &mut impl Rng) -> Roll {
    let a = Die::new(rng.random_range(1..=6));
    let b = Die::new(rng.random_range(1..=6));
    Roll::new(a, b)
}

/// Plays a random legal game from the starting position for `turns` turns (a
/// "turn" = one player's roll-and-play) and returns the resulting position,
/// from the perspective of whoever is on roll when play stops.
///
/// If the game finishes (someone bears off all 15 checkers) before `turns`
/// is reached, starts over from the beginning: a finished game has no legal
/// continuation for anyone, so it is useless as a move-generation fixture.
pub(crate) fn random_reachable_position(rng: &mut impl Rng, turns: u32) -> Position {
    loop {
        let mut position = Position::starting();
        let mut finished = false;

        for _ in 0..turns {
            let roll = random_roll(rng);
            // A roll with no legal reply (e.g. entry fully blocked) still
            // passes the turn; the position itself doesn't change, only the
            // perspective does.
            if let Some(ply) = position.generate_moves(roll).choose(rng) {
                position = position.apply(ply);
            }
            position = position.mirror();

            if position.off()[0] == 15 || position.off()[1] == 15 {
                finished = true;
                break;
            }
        }

        if !finished {
            return position;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn checkers_are_conserved(position: &Position) {
        let on_board = |sign: fn(i8) -> bool| -> i32 {
            (0..24)
                .map(|i| position.point(i))
                .filter(|&c| sign(c))
                .map(|c| c.unsigned_abs() as i32)
                .sum()
        };
        let mine = on_board(|c| c > 0) + position.bar()[0] as i32 + position.off()[0] as i32;
        let theirs = on_board(|c| c < 0) + position.bar()[1] as i32 + position.off()[1] as i32;
        assert_eq!(mine, 15, "{position:?}");
        assert_eq!(theirs, 15, "{position:?}");
    }

    #[test]
    fn zero_turns_returns_the_starting_position() {
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(random_reachable_position(&mut rng, 0), Position::starting());
    }

    #[test]
    fn generated_positions_never_have_a_finished_game() {
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..500 {
            let position = random_reachable_position(&mut rng, 60);
            assert_ne!(
                position.off()[0],
                15,
                "returned a finished game: {position:?}"
            );
            assert_ne!(
                position.off()[1],
                15,
                "returned a finished game: {position:?}"
            );
        }
    }

    #[test]
    fn generated_positions_always_conserve_all_30_checkers() {
        let mut rng = StdRng::seed_from_u64(7);
        for turns in [0, 1, 2, 5, 20, 60, 120] {
            for _ in 0..200 {
                checkers_are_conserved(&random_reachable_position(&mut rng, turns));
            }
        }
    }

    #[test]
    fn play_actually_advances_the_game() {
        // Guards against a self-play loop that silently no-ops (e.g. `mirror`
        // or `apply` wired wrong): a few turns in, the position must differ
        // from the starting one.
        let mut rng = StdRng::seed_from_u64(99);
        let moved = (0..200)
            .map(|_| random_reachable_position(&mut rng, 5))
            .filter(|&p| p != Position::starting())
            .count();
        assert!(
            moved > 190,
            "expected nearly all 5-turn games to have moved, got {moved}/200"
        );
    }

    #[test]
    fn some_generated_positions_still_have_all_30_checkers_on_the_board_or_bar() {
        // The "dense, realistic" positions Phase 1 wants for the MAX_MOVES
        // check (CLAUDE.md §0): nobody has borne off yet, so all 15+15
        // checkers are still in play and branching can be wide.
        let mut rng = StdRng::seed_from_u64(123);
        let dense = (0..200)
            .map(|_| random_reachable_position(&mut rng, 8))
            .filter(|p| p.off() == [0, 0])
            .count();
        assert!(
            dense > 150,
            "expected most 8-turn games to still have all checkers in play, got {dense}/200"
        );
    }
}
