//! A bijection between "at most `max_checkers` indistinguishable checkers placed on `P`
//! distinguishable points" and a dense index range `0..count(P, max_checkers)`.
//!
//! This indexes bearoff positions (checker counts per home-board point) as flat arrays for
//! disk storage and mmap lookup. The mapping is pure combinatorics — independent of
//! backgammon rules — so the same code covers both a one-sided database (`P = 6`,
//! `max_checkers = 15`) and, later, a two-sided one (rank each side separately, combine with
//! `side_a * count(..) + side_b`).
//!
//! All-off (`[0; P]`) ranks to index 0; index increases as more checkers remain on the board.

/// Number of distinct ways to place at most `max_checkers` indistinguishable checkers on
/// `points` distinguishable points (the remainder are implicitly off).
pub fn count(points: usize, max_checkers: u8) -> usize {
    binomial(max_checkers as u64 + points as u64, points as u64)
        .try_into()
        .expect("bearoff position count fits in usize for supported (points, max_checkers)")
}

/// Ranks a checker placement into a dense index in `0..count(P, max_checkers)`.
///
/// `checkers[i]` is the number of checkers on point `i`. Checkers not accounted for by
/// `checkers` (`max_checkers - checkers.iter().sum()`) are implicitly off.
///
/// # Panics
/// Panics if `checkers.iter().sum() > max_checkers`.
pub fn rank<const P: usize>(max_checkers: u8, checkers: [u8; P]) -> usize {
    let total: u32 = checkers.iter().map(|&c| c as u32).sum();
    assert!(
        total <= max_checkers as u32,
        "checker count {total} exceeds max_checkers {max_checkers}"
    );

    let mut index = 0usize;
    let mut remaining = max_checkers;
    for (i, &c) in checkers.iter().enumerate() {
        let remaining_points = P - i - 1;
        for v in 0..c {
            index += count(remaining_points, remaining - v);
        }
        remaining -= c;
    }
    index
}

/// Inverse of [`rank`]: recovers the checker placement from a dense index.
///
/// # Panics
/// Panics if `index >= count(P, max_checkers)`.
pub fn unrank<const P: usize>(max_checkers: u8, mut index: usize) -> [u8; P] {
    assert!(
        index < count(P, max_checkers),
        "index {index} out of range for count {}",
        count(P, max_checkers)
    );

    let mut checkers = [0u8; P];
    let mut remaining = max_checkers;
    for (i, slot) in checkers.iter_mut().enumerate() {
        let remaining_points = P - i - 1;
        let mut v = 0u8;
        loop {
            let c = count(remaining_points, remaining - v);
            if index < c {
                break;
            }
            index -= c;
            v += 1;
        }
        *slot = v;
        remaining -= v;
    }
    checkers
}

/// `n` choose `k`, exact for the small `n` bearoff indexing needs.
///
/// Multiply-then-divide in lockstep keeps every intermediate result an exact binomial
/// coefficient (`C(n, i+1) = C(n, i) * (n - i) / (i + 1)`), so no fractional truncation ever
/// occurs despite the division.
fn binomial(n: u64, k: u64) -> u128 {
    let k = k.min(n.saturating_sub(k));
    let mut result: u128 = 1;
    for i in 0..k {
        result = result * (n - i) as u128 / (i + 1) as u128;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_matches_stars_and_bars_for_one_sided_bearoff() {
        // 15 checkers, 6 home-board points: C(21, 6).
        assert_eq!(count(6, 15), 54_264);
    }

    #[test]
    fn count_with_zero_points_is_one() {
        // No points to place on: the only configuration is "everything off".
        assert_eq!(count(0, 15), 1);
    }

    #[test]
    fn rank_of_all_off_is_zero() {
        assert_eq!(rank::<6>(15, [0; 6]), 0);
    }

    #[test]
    fn rank_is_monotonic_for_a_single_point() {
        // points = 1, max = 2: only checkers[0] varies, 0..=2.
        assert_eq!(rank::<1>(2, [0]), 0);
        assert_eq!(rank::<1>(2, [1]), 1);
        assert_eq!(rank::<1>(2, [2]), 2);
    }

    #[test]
    #[should_panic(expected = "exceeds max_checkers")]
    fn rank_panics_on_too_many_checkers() {
        rank::<1>(2, [3]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn unrank_panics_on_out_of_range_index() {
        let _ = unrank::<1>(2, 3);
    }

    /// Exhaustively checks `rank(unrank(i)) == i` for every `i` in `0..count(P, max_checkers)`,
    /// for a fixed `P` given as a const generic (const generics can't vary at runtime, so
    /// callers instantiate this once per `P` they want covered).
    fn assert_round_trip_exhaustive<const P: usize>(max_checkers: u8) {
        let total = count(P, max_checkers);
        let mut seen = vec![false; total];
        for (index, seen_at_index) in seen.iter_mut().enumerate() {
            let checkers: [u8; P] = unrank(max_checkers, index);
            let sum: u32 = checkers.iter().map(|&c| c as u32).sum();
            assert!(sum <= max_checkers as u32);
            let back = rank(max_checkers, checkers);
            assert_eq!(back, index, "round trip failed for {checkers:?}");
            *seen_at_index = true;
        }
        assert!(
            seen.into_iter().all(|s| s),
            "not all indices reached for P={P}, max_checkers={max_checkers}"
        );
    }

    #[test]
    fn rank_and_unrank_round_trip_exhaustively_for_small_cases() {
        for max_checkers in 0..=6u8 {
            assert_round_trip_exhaustive::<0>(max_checkers);
            assert_round_trip_exhaustive::<1>(max_checkers);
            assert_round_trip_exhaustive::<2>(max_checkers);
            assert_round_trip_exhaustive::<3>(max_checkers);
            assert_round_trip_exhaustive::<4>(max_checkers);
        }
    }

    #[test]
    fn rank_and_unrank_round_trip_exhaustively_for_one_sided_bearoff_shape() {
        // The real Phase 2 shape: 15 checkers, 6 home-board points. 54,264 is small enough
        // to check every index, not a sample.
        let total = count(6, 15);
        for index in 0..total {
            let checkers: [u8; 6] = unrank(15, index);
            let sum: u32 = checkers.iter().map(|&c| c as u32).sum();
            assert!(sum <= 15);
            assert_eq!(
                rank(15, checkers),
                index,
                "round trip failed for {checkers:?}"
            );
        }
    }
}
