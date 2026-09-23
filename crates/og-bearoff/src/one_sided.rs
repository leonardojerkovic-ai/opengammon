//! The one-sided bearoff database: 15 checkers, 6 home-board points, computed by backward
//! dynamic programming. See `OPENGAMMON.md` Phase 2 and `docs/rules-notes.md` for the
//! "minimize own expected rolls" limitation this database's values are built on, and for
//! why `finish` and `first_off` are optimized under two different policies rather than
//! one, confirmed against GNUbg on the worst-case position.

use og_core::{Die, Ply, Position, Roll};

use crate::combinatorial;

/// Home-board points this database covers.
pub const POINTS: usize = 6;
/// Checkers per side.
pub const MAX_CHECKERS: u8 = 15;

/// Per-position statistics. `finish` and `first_off` are each optimized under their own
/// policy (see [`finish_score`] and [`first_off_score`]) — not the same policy, and
/// neither is globally optimal play (see the module doc).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `finish[r]` = P(exactly `r` rolls left to bear off the *last* checker),
    /// `r = 0..finish.len()`. The terminal (all-off) position has `finish == [1.0]`.
    pub finish: Vec<f64>,
    /// `first_off[i]` = P(exactly `i + 1` rolls left to bear off the *first* checker),
    /// for gammon calculations (bearing off the first checker always takes at least one
    /// roll, so there's no index for "zero rolls" to waste).
    ///
    /// **Empty for every position except one with all `MAX_CHECKERS` still on board
    /// (`off == 0`)**, not just the terminal one — GNUbg's own "saving gammon"
    /// statistic is retroactive, not prospective: it asks whether a checker has
    /// *already* come off, which becomes trivially true (100% at 0 rolls, no further
    /// computation needed) the instant `off > 0`. Storing that constant for 38,760 of
    /// the 54,264 positions (everywhere `off > 0`) would be pure waste — see
    /// `docs/rules-notes.md`. Callers that need a value for an `off > 0` position
    /// already know it without a lookup: "already saved".
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

/// Picks whichever `plies` has the lowest `score` (a function of the resulting
/// position's rank index and whether that play bore off at least one checker).
/// Shared by every selection policy below (the DP build, for both statistics, and the
/// Monte Carlo validation) so they can't silently drift apart in the *mechanics* of
/// picking a play — only in the `score` each one passes in.
///
/// Ties are broken by lowest resulting rank index, arbitrarily but deterministically.
///
/// # Panics
/// Panics if `plies` is empty.
fn choose_best_ply(
    position: &Position,
    total_checkers: u32,
    plies: &[Ply],
    score: impl Fn(usize, bool) -> f64,
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
        let bore_off = resulting_total < total_checkers;
        let value = score(child_index, bore_off);

        let better = match &best {
            None => true,
            Some((_, best_index, _, best_value)) => {
                value < *best_value || (value == *best_value && child_index < *best_index)
            }
        };
        if better {
            best = Some((resulting_checkers, child_index, bore_off, value));
        }
    }
    let (checkers, _, bore_off, _) = best.expect("plies is non-empty");
    (checkers, bore_off)
}

/// Score for the `finish` policy: minimize the resulting position's own expected
/// total rolls to bear off everything.
fn finish_score(entries: &[Option<Entry>], child_index: usize, _bore_off: bool) -> f64 {
    mean(&entries[child_index]
        .as_ref()
        .unwrap_or_else(|| {
            panic!("child {child_index} has strictly fewer pips than its parent and should already be computed")
        })
        .finish)
}

/// Score for the `first_off` policy: a *separate* optimization from `finish` (this one
/// models a player trying to save a gammon, who wants a checker off *now* rather than
/// racing efficiently). Bearing off this roll always scores 1 (one roll used, done);
/// not bearing off scores 1 + the child's own expected rolls to its first bear-off,
/// recursively minimized the same way. Since 1 is never worse than 1 + a non-negative
/// quantity, this reduces to: bear off if any play can, otherwise pick whichever
/// non-bearing-off child gets a checker off soonest.
///
/// Confirmed against GNUbg's own `gnubg_os0.bd`, not just assumed: for the worst-case
/// position (15 checkers on point 6, via `bearoffdump.exe`), this exact policy
/// reproduces GNUbg's `first_off` percentages and mean (1.616) to the precision GNUbg
/// displays, while the earlier single-shared-policy version (using `finish_score` for
/// both statistics) was measurably off (mean 1.700). See `docs/rules-notes.md`.
fn first_off_score(entries: &[Option<Entry>], child_index: usize, bore_off: bool) -> f64 {
    if bore_off {
        return 1.0;
    }
    let child = entries[child_index].as_ref().unwrap_or_else(|| {
        panic!("child {child_index} has strictly fewer pips than its parent and should already be computed")
    });
    1.0 + mean(&child.first_off)
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

    // GNUbg's own "saving gammon" statistic is retroactive, not prospective: it asks
    // whether a checker has *already* come off (true the instant `off > 0`), not how
    // many more rolls until the next one. So first_off only carries information for
    // off == 0 positions (all MAX_CHECKERS still on board, 15,504 of the 54,264 total
    // — see docs/rules-notes.md); everywhere else the answer is the known constant
    // "already saved" and isn't computed or stored.
    let needs_first_off = total_checkers == MAX_CHECKERS as u32;

    // finish and first_off (when needed) are optimized separately, under two
    // different policies (see first_off_score's doc): a roll's best play for racing
    // speed and its best play for gammon-saving speed can be different plays. Every
    // child referenced by either has strictly fewer pips than this position, so it's
    // already in `entries`.
    let mut finish_chosen: Vec<usize> = Vec::with_capacity(rolls.len());
    let mut first_off_chosen: Vec<(usize, bool)> =
        Vec::with_capacity(if needs_first_off { rolls.len() } else { 0 });
    for &(roll, _weight) in rolls {
        let plies = position.generate_moves(roll);
        assert!(
            !plies.is_empty(),
            "a one-sided position with checkers remaining and no opponent always has a legal move"
        );

        let (finish_checkers, _) =
            choose_best_ply(&position, total_checkers, &plies, |index, bore_off| {
                finish_score(entries, index, bore_off)
            });
        finish_chosen.push(combinatorial::rank(MAX_CHECKERS, finish_checkers));

        if needs_first_off {
            let (first_off_checkers, bore_off) =
                choose_best_ply(&position, total_checkers, &plies, |index, bore_off| {
                    first_off_score(entries, index, bore_off)
                });
            first_off_chosen.push((
                combinatorial::rank(MAX_CHECKERS, first_off_checkers),
                bore_off,
            ));
        }
    }

    let mut finish_len = 1;
    for &child_index in &finish_chosen {
        finish_len = finish_len.max(entries[child_index].as_ref().unwrap().finish.len() + 1);
    }
    let mut finish = vec![0.0f64; finish_len];
    for (&(_, weight), &child_index) in rolls.iter().zip(finish_chosen.iter()) {
        let child = entries[child_index].as_ref().unwrap();
        for (r, &p) in child.finish.iter().enumerate() {
            finish[r + 1] += weight * p;
        }
    }

    let first_off = if !needs_first_off {
        Vec::new()
    } else {
        let mut first_off_len = 0;
        for &(child_index, bore_off) in &first_off_chosen {
            if bore_off {
                first_off_len = first_off_len.max(1);
            } else {
                let child = entries[child_index].as_ref().unwrap();
                first_off_len = first_off_len.max(child.first_off.len() + 1);
            }
        }
        let mut first_off = vec![0.0f64; first_off_len];
        for (&(_, weight), &(child_index, bore_off)) in rolls.iter().zip(first_off_chosen.iter()) {
            if bore_off {
                first_off[0] += weight;
            } else {
                let child = entries[child_index].as_ref().unwrap();
                for (r, &p) in child.first_off.iter().enumerate() {
                    first_off[r + 1] += weight * p;
                }
            }
        }
        first_off
    };

    Entry { finish, first_off }
}

/// Positions where at least one of the 21 rolls has two or more legal plies whose
/// `finish_score` differ by less than `threshold` — a near-tie our arbitrary
/// lowest-rank-index tie-break could plausibly resolve differently than GNUbg's own
/// construction does. `pub(crate)` for `gnubg_diff.rs`'s exhaustive comparison, which
/// uses this to tell a position's *own* near-tie apart from a deviation merely
/// inherited through the DP from some ancestor's near-tie. See `docs/rules-notes.md`.
#[cfg(test)]
pub(crate) fn positions_with_near_tied_finish_choice(
    table: &[Entry],
    threshold: f64,
) -> std::collections::HashSet<usize> {
    let entries: Vec<Option<Entry>> = table.iter().cloned().map(Some).collect();
    let rolls = all_rolls_with_weights();
    let mut result = std::collections::HashSet::new();

    for (index, _) in table.iter().enumerate() {
        let checkers: [u8; POINTS] = combinatorial::unrank(MAX_CHECKERS, index);
        let total_checkers: u32 = checkers.iter().map(|&c| c as u32).sum();
        if total_checkers == 0 {
            continue;
        }
        let position = position_for(checkers);

        for &(roll, _weight) in &rolls {
            let plies = position.generate_moves(roll);
            if plies.len() < 2 {
                continue;
            }
            let mut scores: Vec<f64> = plies
                .iter()
                .map(|ply| {
                    let resulting = position.apply(ply);
                    let mut rc = [0u8; POINTS];
                    for (i, slot) in rc.iter_mut().enumerate() {
                        *slot = resulting.point(i) as u8;
                    }
                    let idx = combinatorial::rank(MAX_CHECKERS, rc);
                    let bore_off = rc.iter().map(|&c| c as u32).sum::<u32>() < total_checkers;
                    finish_score(&entries, idx, bore_off)
                })
                .collect();
            scores.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if scores[1] - scores[0] < threshold {
                result.insert(index);
                break;
            }
        }
    }

    result
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
        // Only 1 of 15 checkers on the board (14 already off): first_off is
        // deliberately not stored here (GNUbg's "saving gammon" is already trivially
        // true the instant off > 0) even though finish is a real computation.
        let entry = lookup(table(), checkers(&[(1, 1)]));
        assert_close_vec(&entry.finish, &[0.0, 1.0], "finish");
        assert!(entry.first_off.is_empty());
    }

    #[test]
    fn two_checkers_on_point_one_both_bear_off_in_exactly_one_roll() {
        // Any roll has two dice, and a checker on point 1 always bears off with any
        // single die value, so both checkers always clear in the very first roll.
        // Still off > 0 (13 already off), so first_off is empty, same as above.
        let entry = lookup(table(), checkers(&[(1, 2)]));
        assert_close_vec(&entry.finish, &[0.0, 1.0], "finish");
        assert!(entry.first_off.is_empty());
    }

    #[test]
    fn finish_distribution_sums_to_one_for_every_position() {
        for (index, entry) in table().iter().enumerate() {
            let sum: f64 = entry.finish.iter().sum();
            assert_close(sum, 1.0, &format!("finish distribution at index {index}"));
        }
    }

    #[test]
    fn first_off_is_stored_only_for_positions_with_all_checkers_still_on_board() {
        // 15,504 = C(20, 5): the number of ways to place exactly 15 (not "at most 15")
        // checkers on 6 points — see docs/rules-notes.md for why only these positions
        // carry a real first_off distribution.
        const EXPECTED_OFF_ZERO_COUNT: usize = 15_504;
        let mut off_zero_count = 0;

        for (index, entry) in table().iter().enumerate() {
            let checkers: [u8; POINTS] = combinatorial::unrank(MAX_CHECKERS, index);
            let total: u32 = checkers.iter().map(|&c| c as u32).sum();

            if total == MAX_CHECKERS as u32 {
                off_zero_count += 1;
                let sum: f64 = entry.first_off.iter().sum();
                assert_close(
                    sum,
                    1.0,
                    &format!("first_off distribution at index {index}"),
                );
            } else {
                assert!(
                    entry.first_off.is_empty(),
                    "index {index} (total {total}) should have empty first_off, got {:?}",
                    entry.first_off
                );
            }
        }

        assert_eq!(off_zero_count, EXPECTED_OFF_ZERO_COUNT);
    }

    #[test]
    fn worst_position_first_off_matches_gnubg() {
        // Regression test for the two-policy fix: confirmed against GNUbg's own
        // gnubg_os0.bd via bearoffdump.exe on 2026-09-23 (see docs/rules-notes.md).
        // The expected values are only known to 6 decimals (read off our own printed
        // output, already checked against GNUbg's 3-decimal display), so this uses a
        // looser tolerance than assert_close_vec's 1e-9 — tight enough to catch a
        // regression in the two-policy logic, loose enough not to fail on the last
        // couple of digits of a value nobody claimed was exact.
        let entry = lookup(table(), checkers(&[(6, 15)]));
        let expected = [
            0.472222, 0.449846, 0.068651, 0.008223, 0.000974, 0.000081, 0.000003, 0.0,
        ];
        assert_eq!(entry.first_off.len(), expected.len());
        for (i, (&actual, &expected)) in entry.first_off.iter().zip(&expected).enumerate() {
            assert!(
                (actual - expected).abs() < 1e-5,
                "first_off[{i}]: {actual} != {expected}"
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
    /// completion many times with real random dice, using the exact same per-statistic
    /// policy ([`choose_best_ply`] with [`finish_score`] or [`first_off_score`]) the DP
    /// used to build the table, and count rolls. If the DP has a bug, this measures the
    /// true distribution of that policy by brute force and won't reproduce the DP's
    /// (wrong) answer — unlike comparing the DP against GNUbg, this doesn't depend on
    /// the DP being right about anything, only on `generate_moves` and `choose_best_ply`
    /// being correct, which the simulation exercises directly.
    ///
    /// `finish` and `first_off` are optimized under two *different* policies (see
    /// `first_off_score`'s doc), so they're validated with two separate playouts of the
    /// same starting position, not one shared trajectory: a single played-out game can
    /// only ever follow one policy's choices at a time.
    mod monte_carlo_validation {
        use rand::rngs::StdRng;
        use rand::{RngExt, SeedableRng};

        use super::*;

        fn random_roll(rng: &mut StdRng) -> Roll {
            Roll::new(
                Die::new(rng.random_range(1..=6)),
                Die::new(rng.random_range(1..=6)),
            )
        }

        /// Plays `start` to completion once under the `finish`-minimizing policy,
        /// returning the number of rolls used.
        fn play_for_finish(
            start: [u8; POINTS],
            entries: &[Option<Entry>],
            rng: &mut StdRng,
        ) -> u32 {
            let mut checkers = start;
            let mut total: u32 = checkers.iter().map(|&c| c as u32).sum();
            let mut rolls = 0u32;

            while total > 0 {
                rolls += 1;
                let position = position_for(checkers);
                let plies = position.generate_moves(random_roll(rng));
                assert!(!plies.is_empty(), "checkers remain, so a legal move exists");

                let (next_checkers, _) =
                    choose_best_ply(&position, total, &plies, |index, bore_off| {
                        finish_score(entries, index, bore_off)
                    });
                checkers = next_checkers;
                total = checkers.iter().map(|&c| c as u32).sum();
            }
            rolls
        }

        /// Plays `start` under the `first_off`-minimizing (gammon-saving) policy until
        /// the first checker comes off, returning the number of rolls used.
        fn play_for_first_off(
            start: [u8; POINTS],
            entries: &[Option<Entry>],
            rng: &mut StdRng,
        ) -> u32 {
            let mut checkers = start;
            let mut total: u32 = checkers.iter().map(|&c| c as u32).sum();
            let mut rolls = 0u32;

            loop {
                rolls += 1;
                let position = position_for(checkers);
                let plies = position.generate_moves(random_roll(rng));
                assert!(!plies.is_empty(), "checkers remain, so a legal move exists");

                let (next_checkers, bore_off) =
                    choose_best_ply(&position, total, &plies, |index, bore_off| {
                        first_off_score(entries, index, bore_off)
                    });
                if bore_off {
                    return rolls;
                }
                checkers = next_checkers;
                total = checkers.iter().map(|&c| c as u32).sum();
            }
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

        /// Asserts an empirical mean (from `trials` simulated rolls-counts) is within
        /// `tolerance_sigmas` standard errors of `exact_mean`/`exact_var`. A
        /// deliberately loose tolerance: this is a cheap sanity check against gross DP
        /// bugs, not a precise statistical test. The epsilon floor covers a
        /// deterministic exact distribution (variance exactly 0): without it, even a
        /// perfect match (diff == 0) would fail `< 0`.
        fn assert_matches_exact_mean(
            empirical_mean: f64,
            exact_mean: f64,
            exact_var: f64,
            trials: u32,
            label: &str,
        ) {
            let tolerance_sigmas = 5.0;
            let threshold = tolerance_sigmas * (exact_var / trials as f64).sqrt() + 1e-9;
            assert!(
                (empirical_mean - exact_mean).abs() < threshold,
                "{label}: simulated {empirical_mean} vs exact {exact_mean} \
                 (threshold {threshold}, {trials} trials)"
            );
        }

        fn assert_simulation_matches_dp(start: [u8; POINTS], trials: u32, seed: u64) {
            let table = table();
            let entries: Vec<Option<Entry>> = table.iter().cloned().map(Some).collect();
            let entry = lookup(table, start);
            let (exact_finish_mean, exact_finish_var) = mean_and_variance(&entry.finish, 0.0);
            let (exact_first_off_mean, exact_first_off_var) =
                mean_and_variance(&entry.first_off, 1.0);

            let mut rng = StdRng::seed_from_u64(seed);
            let mut finish_sum = 0u64;
            let mut first_off_sum = 0u64;
            for _ in 0..trials {
                finish_sum += play_for_finish(start, &entries, &mut rng) as u64;
                first_off_sum += play_for_first_off(start, &entries, &mut rng) as u64;
            }

            assert_matches_exact_mean(
                finish_sum as f64 / trials as f64,
                exact_finish_mean,
                exact_finish_var,
                trials,
                "finish mean",
            );
            assert_matches_exact_mean(
                first_off_sum as f64 / trials as f64,
                exact_first_off_mean,
                exact_first_off_var,
                trials,
                "first_off mean",
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
