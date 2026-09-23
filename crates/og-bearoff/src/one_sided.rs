//! The one-sided bearoff database: 15 checkers, 6 home-board points, computed by backward
//! dynamic programming. See `OPENGAMMON.md` Phase 2 and `docs/rules-notes.md` for the
//! "minimize own expected rolls" limitation this database's values are built on.

use og_core::{Die, Ply, Position, Roll};

use crate::combinatorial;

/// Home-board points this database covers.
pub const POINTS: usize = 6;
/// Checkers per side.
pub const MAX_CHECKERS: u8 = 15;

/// Per-position statistics, both derived from a single policy: at every roll, play
/// whichever legal ply minimizes the expected number of rolls left to bear off all of
/// this side's checkers (see the module doc for why that's not the same as globally
/// optimal play).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `finish[r]` = P(exactly `r` rolls left to bear off the *last* checker),
    /// `r = 0..finish.len()`. The terminal (all-off) position has `finish == [1.0]`.
    pub finish: Vec<f64>,
    /// `first_off[i]` = P(exactly `i + 1` rolls left to bear off the *first* checker),
    /// for gammon calculations (bearing off the first checker always takes at least one
    /// roll, so there's no index for "zero rolls" to waste). Empty for the terminal
    /// position, which has no future "first bear-off" event to wait for.
    pub first_off: Vec<f64>,
}

/// Computes the full one-sided database: one [`Entry`] per position, indexed by
/// [`combinatorial::rank`] (`POINTS` points, `MAX_CHECKERS` checkers).
pub fn compute_table() -> Vec<Entry> {
    let total = combinatorial::count(POINTS, MAX_CHECKERS);
    let mut entries: Vec<Option<Entry>> = vec![None; total];

    // Every legal play reduces total pips by at least 1 (bearing off past a point
    // costs that point's own pips; moving costs the die value), so a position's
    // children always have strictly fewer pips than the position itself. Bucketing by
    // pip count and processing buckets in increasing order guarantees every child is
    // already computed by the time its parent needs it.
    let max_pips = POINTS as u32 * MAX_CHECKERS as u32;
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); max_pips as usize + 1];
    for index in 0..total {
        let checkers: [u8; POINTS] = combinatorial::unrank(MAX_CHECKERS, index);
        buckets[pip_sum(&checkers) as usize].push(index);
    }

    let rolls = all_rolls_with_weights();

    for bucket in &buckets {
        for &index in bucket {
            let checkers: [u8; POINTS] = combinatorial::unrank(MAX_CHECKERS, index);
            entries[index] = Some(compute_entry(checkers, &rolls, &entries));
        }
    }

    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            entry.unwrap_or_else(|| {
                panic!("position {index} never computed: every index is visited exactly once, in pip-count order")
            })
        })
        .collect()
}

/// Looks up the entry for a checker placement in a table returned by [`compute_table`].
pub fn lookup(table: &[Entry], checkers: [u8; POINTS]) -> &Entry {
    &table[combinatorial::rank(MAX_CHECKERS, checkers)]
}

fn pip_sum(checkers: &[u8; POINTS]) -> u32 {
    checkers
        .iter()
        .enumerate()
        .map(|(i, &c)| (i as u32 + 1) * c as u32)
        .sum()
}

fn mean(distribution: &[f64]) -> f64 {
    distribution
        .iter()
        .enumerate()
        .map(|(r, &p)| r as f64 * p)
        .sum()
}

/// All 21 distinct rolls with their probability weight (2/36 for non-doubles, 1/36 for
/// doubles): the standard enumeration used throughout this workspace (see
/// `og-core`'s `gnubg_diff` module).
fn all_rolls_with_weights() -> Vec<(Roll, f64)> {
    let mut rolls = Vec::with_capacity(21);
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let weight = if d1 == d2 { 1.0 / 36.0 } else { 2.0 / 36.0 };
            rolls.push((Roll::new(Die::new(d1), Die::new(d2)), weight));
        }
    }
    rolls
}

/// Builds the (partial, one-sided) `Position` for a home-board checker placement: only
/// `checkers` on points 1..=6, nothing else, no opponent anywhere.
fn position_for(checkers: [u8; POINTS]) -> Position {
    let total: u32 = checkers.iter().map(|&c| c as u32).sum();
    let mut points = [0i8; 24];
    for (i, &c) in checkers.iter().enumerate() {
        points[i] = c as i8;
    }
    Position::from_raw(points, [0, 0], [(MAX_CHECKERS as u32 - total) as u8, 0]).expect(
        "one-sided bearoff positions have at most MAX_CHECKERS on one side and none on the other, by construction",
    )
}

/// Picks whichever `plies` leads to the position with the lowest expected rolls left
/// to finish, per `child_mean` (a lookup from a resulting position's rank index to its
/// already-known expected-rolls value). Shared between the DP build (where `child_mean`
/// reads from the table-in-progress) and the Monte Carlo validation below (where it
/// reads from the finished table) so both use the exact same selection policy.
///
/// Ties are broken by lowest resulting rank index, arbitrarily but deterministically: a
/// genuine tie in expected value is expected to be a tie in the full distribution too
/// for pure bearoff (see `docs/rules-notes.md`), so the tie-break shouldn't matter in
/// practice.
///
/// # Panics
/// Panics if `plies` is empty.
fn choose_best_ply(
    position: &Position,
    total_checkers: u32,
    plies: &[Ply],
    child_mean: impl Fn(usize) -> f64,
) -> ([u8; POINTS], bool) {
    let mut best: Option<([u8; POINTS], usize, bool, f64)> = None;
    for ply in plies {
        let resulting = position.apply(ply);
        let mut resulting_checkers = [0u8; POINTS];
        for (i, slot) in resulting_checkers.iter_mut().enumerate() {
            let c = resulting.point(i);
            debug_assert!(
                c >= 0,
                "one-sided position never produces opponent checkers"
            );
            *slot = c as u8;
        }
        let resulting_total: u32 = resulting_checkers.iter().map(|&c| c as u32).sum();
        let child_index = combinatorial::rank(MAX_CHECKERS, resulting_checkers);
        let expected = child_mean(child_index);
        let bore_off = resulting_total < total_checkers;

        let better = match &best {
            None => true,
            Some((_, best_index, _, best_expected)) => {
                expected < *best_expected
                    || (expected == *best_expected && child_index < *best_index)
            }
        };
        if better {
            best = Some((resulting_checkers, child_index, bore_off, expected));
        }
    }
    let (checkers, _, bore_off, _) = best.expect("plies is non-empty");
    (checkers, bore_off)
}

fn compute_entry(
    checkers: [u8; POINTS],
    rolls: &[(Roll, f64)],
    entries: &[Option<Entry>],
) -> Entry {
    let total_checkers: u32 = checkers.iter().map(|&c| c as u32).sum();
    if total_checkers == 0 {
        return Entry {
            finish: vec![1.0],
            first_off: Vec::new(),
        };
    }

    let position = position_for(checkers);

    // For each roll, pick the legal play minimizing the expected number of rolls left
    // to finish, using children's already-computed `finish` means — every child has
    // strictly fewer pips, so it's already in `entries`.
    let mut chosen: Vec<(usize, bool)> = Vec::with_capacity(rolls.len());
    for &(roll, _weight) in rolls {
        let plies = position.generate_moves(roll);
        assert!(
            !plies.is_empty(),
            "a one-sided position with checkers remaining and no opponent always has a legal move"
        );

        let (child_checkers, bore_off) = choose_best_ply(
            &position,
            total_checkers,
            &plies,
            |index| {
                mean(&entries[index]
                    .as_ref()
                    .unwrap_or_else(|| {
                        panic!("child {index} has strictly fewer pips than its parent and should already be computed")
                    })
                    .finish)
            },
        );
        chosen.push((combinatorial::rank(MAX_CHECKERS, child_checkers), bore_off));
    }

    let mut finish_len = 1;
    let mut first_off_len = 0;
    for &(child_index, bore_off) in &chosen {
        let child = entries[child_index].as_ref().unwrap();
        finish_len = finish_len.max(child.finish.len() + 1);
        if bore_off {
            first_off_len = first_off_len.max(1);
        } else if !child.first_off.is_empty() {
            first_off_len = first_off_len.max(child.first_off.len() + 1);
        }
    }

    let mut finish = vec![0.0f64; finish_len];
    let mut first_off = vec![0.0f64; first_off_len];
    for (&(roll, weight), &(child_index, bore_off)) in rolls.iter().zip(chosen.iter()) {
        let _ = roll;
        let child = entries[child_index].as_ref().unwrap();
        for (r, &p) in child.finish.iter().enumerate() {
            finish[r + 1] += weight * p;
        }
        if bore_off {
            first_off[0] += weight;
        } else {
            for (r, &p) in child.first_off.iter().enumerate() {
                first_off[r + 1] += weight * p;
            }
        }
    }

    Entry { finish, first_off }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;

    /// The full table is expensive enough (real move generation over 54,264 positions
    /// x 21 rolls) that every test sharing one build instead of computing its own
    /// matters for suite runtime.
    fn table() -> &'static [Entry] {
        static TABLE: OnceLock<Vec<Entry>> = OnceLock::new();
        TABLE.get_or_init(compute_table)
    }

    fn checkers(points: &[(usize, u8)]) -> [u8; POINTS] {
        let mut c = [0u8; POINTS];
        for &(point, count) in points {
            c[point - 1] = count;
        }
        c
    }

    fn assert_close(actual: f64, expected: f64, msg: &str) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{msg}: {actual} != {expected}"
        );
    }

    fn assert_close_vec(actual: &[f64], expected: &[f64], msg: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{msg}: length {} != {}",
            actual.len(),
            expected.len()
        );
        for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
            assert_close(a, e, &format!("{msg}[{i}]"));
        }
    }

    #[test]
    fn terminal_position_has_point_mass_at_zero_rolls() {
        let entries = vec![Some(Entry {
            finish: vec![1.0],
            first_off: Vec::new(),
        })];
        // compute_entry for the all-off position doesn't need real children.
        let entry = compute_entry([0; POINTS], &[], &entries);
        assert_eq!(entry.finish, vec![1.0]);
        assert!(entry.first_off.is_empty());
    }

    #[test]
    fn single_checker_on_point_one_bears_off_in_exactly_one_roll() {
        let entry = lookup(table(), checkers(&[(1, 1)]));
        assert_close_vec(&entry.finish, &[0.0, 1.0], "finish");
        assert_close_vec(&entry.first_off, &[1.0], "first_off");
    }

    #[test]
    fn two_checkers_on_point_one_both_bear_off_in_exactly_one_roll() {
        // Any roll has two dice, and a checker on point 1 always bears off with any
        // single die value, so both checkers always clear in the very first roll.
        let entry = lookup(table(), checkers(&[(1, 2)]));
        assert_close_vec(&entry.finish, &[0.0, 1.0], "finish");
        assert_close_vec(&entry.first_off, &[1.0], "first_off");
    }

    #[test]
    fn finish_distribution_sums_to_one_for_every_position() {
        for (index, entry) in table().iter().enumerate() {
            let sum: f64 = entry.finish.iter().sum();
            assert_close(sum, 1.0, &format!("finish distribution at index {index}"));
        }
    }

    #[test]
    fn first_off_distribution_sums_to_one_for_every_non_terminal_position() {
        for (index, entry) in table().iter().enumerate() {
            if entry.finish == [1.0] {
                continue; // terminal position: first_off is deliberately empty
            }
            let sum: f64 = entry.first_off.iter().sum();
            assert_close(
                sum,
                1.0,
                &format!("first_off distribution at index {index}"),
            );
        }
    }

    #[test]
    fn worst_position_has_the_largest_finish_support() {
        // 15 checkers on point 6 is the worst (highest-pip) one-sided position: it
        // should need at least as many rolls, in the worst case, as any other.
        let worst = lookup(table(), checkers(&[(6, 15)]));
        let worst_support = worst.finish.len();
        for entry in table() {
            assert!(entry.finish.len() <= worst_support);
        }
    }

    /// Cross-checks the DP against an independent method: actually play a position to
    /// completion many times with real random dice, using the exact same policy
    /// ([`choose_best_ply`]) the DP used to build the table, and count rolls. If the DP
    /// has a bug, this measures the true distribution of that policy by brute force and
    /// won't reproduce the DP's (wrong) answer — unlike comparing the DP against GNUbg,
    /// this doesn't depend on the DP being right about anything, only on `generate_moves`
    /// and `choose_best_ply` being correct, which the simulation exercises directly.
    mod monte_carlo_validation {
        use rand::rngs::StdRng;
        use rand::{RngExt, SeedableRng};

        use super::*;

        /// Plays `start` to completion once, returning (rolls to bear off the first
        /// checker, rolls to bear off the last checker).
        fn play_once(start: [u8; POINTS], table: &[Entry], rng: &mut StdRng) -> (u32, u32) {
            let mut checkers = start;
            let mut total: u32 = checkers.iter().map(|&c| c as u32).sum();
            let mut rolls = 0u32;
            let mut first_off = None;

            while total > 0 {
                rolls += 1;
                let roll = Roll::new(
                    Die::new(rng.random_range(1..=6)),
                    Die::new(rng.random_range(1..=6)),
                );
                let position = position_for(checkers);
                let plies = position.generate_moves(roll);
                assert!(!plies.is_empty(), "checkers remain, so a legal move exists");

                let (next_checkers, bore_off) =
                    choose_best_ply(&position, total, &plies, |index| mean(&table[index].finish));
                if bore_off && first_off.is_none() {
                    first_off = Some(rolls);
                }
                checkers = next_checkers;
                total = checkers.iter().map(|&c| c as u32).sum();
            }

            (
                first_off.expect("total reached 0, so at least one bear-off happened"),
                rolls,
            )
        }

        /// Mean and variance of a rolls-needed distribution, where index `i` represents
        /// `i + roll_offset` rolls: `roll_offset` is 0 for `finish` (index r = r rolls)
        /// and 1 for `first_off` (index 0 = 1 roll, per its documented convention).
        fn mean_and_variance(distribution: &[f64], roll_offset: f64) -> (f64, f64) {
            let m = mean(distribution) + roll_offset;
            let var: f64 = distribution
                .iter()
                .enumerate()
                .map(|(i, &p)| (i as f64 + roll_offset - m).powi(2) * p)
                .sum();
            (m, var)
        }

        /// Runs `trials` simulated games from `start` and asserts the empirical mean
        /// number of rolls (to first and to last checker off) is within
        /// `tolerance_sigmas` standard errors of the DP's exact mean. A deliberately
        /// loose tolerance: this is a cheap sanity check against gross DP bugs, not a
        /// precise statistical test.
        fn assert_simulation_matches_dp(start: [u8; POINTS], trials: u32, seed: u64) {
            let table = table();
            let entry = lookup(table, start);
            let (exact_finish_mean, exact_finish_var) = mean_and_variance(&entry.finish, 0.0);
            let (exact_first_off_mean, exact_first_off_var) =
                mean_and_variance(&entry.first_off, 1.0);

            let mut rng = StdRng::seed_from_u64(seed);
            let mut finish_sum = 0u64;
            let mut first_off_sum = 0u64;
            for _ in 0..trials {
                let (first_off, finish) = play_once(start, table, &mut rng);
                finish_sum += finish as u64;
                first_off_sum += first_off as u64;
            }
            let empirical_finish_mean = finish_sum as f64 / trials as f64;
            let empirical_first_off_mean = first_off_sum as f64 / trials as f64;

            let tolerance_sigmas = 5.0;
            // The epsilon floor covers a deterministic exact distribution (variance
            // exactly 0, e.g. a position that always finishes in exactly N rolls):
            // without it, even a perfect match (diff == 0) would fail `< 0`.
            let finish_threshold =
                tolerance_sigmas * (exact_finish_var / trials as f64).sqrt() + 1e-9;
            let first_off_threshold =
                tolerance_sigmas * (exact_first_off_var / trials as f64).sqrt() + 1e-9;

            assert!(
                (empirical_finish_mean - exact_finish_mean).abs() < finish_threshold,
                "finish mean: simulated {empirical_finish_mean} vs exact {exact_finish_mean} \
                 (threshold {finish_threshold}, {trials} trials)"
            );
            assert!(
                (empirical_first_off_mean - exact_first_off_mean).abs() < first_off_threshold,
                "first_off mean: simulated {empirical_first_off_mean} vs exact {exact_first_off_mean} \
                 (threshold {first_off_threshold}, {trials} trials)"
            );
        }

        #[test]
        #[ignore = "slow: tens of thousands of simulated games"]
        fn matches_dp_for_the_worst_position() {
            assert_simulation_matches_dp(checkers(&[(6, 15)]), 50_000, 0xb0ad1ce);
        }

        #[test]
        #[ignore = "slow: tens of thousands of simulated games"]
        fn matches_dp_for_a_spread_position() {
            // 2 checkers on each of the 6 points, 3 extra on point 6: sums to 15,
            // structurally different from the fully stacked worst case.
            assert_simulation_matches_dp(
                checkers(&[(1, 2), (2, 2), (3, 2), (4, 2), (5, 2), (6, 5)]),
                50_000,
                0xb0ad1ce,
            );
        }
    }
}
