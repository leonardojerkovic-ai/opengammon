//! Fixed-point quantization of bearoff probability distributions for compact,
//! mmap-friendly storage: each `f64` probability becomes a `u16`, 4x smaller. See
//! `docs/rules-notes.md` for the sum-invariant decision this module implements.

/// Quantization scale. Error per independently-rounded value is at most `1 / SCALE`
/// (~0.0015%), well below the ~0.05% (0.0005 absolute on a 0..100 percentage) precision
/// `gnubg_diff.rs` already compares against.
pub const SCALE: u32 = u16::MAX as u32;

/// Quantizes a probability distribution that sums to 1.0 (within float tolerance) into
/// fixed-point `u16`s that sum to *exactly* `SCALE`.
///
/// Every value except the last is independently rounded (`round(p * SCALE)`); the last
/// is whatever remains (`SCALE` minus the sum of the others), not independently
/// rounded. Deliberately not renormalized at read time instead: this keeps the sum
/// invariant exact in storage, at zero cost, so a lookup never needs a floating-point
/// division to restore it — matching Phase 2's "lookup under a microsecond" target. The
/// remainder is meant to land on the last bucket, which is where every distribution
/// this module quantizes has its smallest, most negligible probability mass (the long
/// tail — see `docs/rules-notes.md`), so that's also where absorbing rounding error
/// matters least.
///
/// Independent rounding of many values can overshoot `SCALE` by a handful of units
/// (confirmed on the real one-sided table, not just in theory — see this module's
/// tests): each of up to ~30 values can round up by as much as 0.5, and those can add
/// up. When that happens, the overshoot is taken back from the *largest* remaining
/// values one unit at a time — negligible relative error there — rather than letting
/// it go to the last bucket, which is the one place a tail distribution can't afford
/// to absorb an error that size.
///
/// # Panics
/// Panics if `distribution` is empty.
pub fn quantize(distribution: &[f64]) -> Vec<u16> {
    assert!(
        !distribution.is_empty(),
        "cannot quantize an empty distribution"
    );
    let (_last, prefix) = distribution.split_last().expect("checked non-empty above");
    let mut quantized: Vec<u32> = prefix
        .iter()
        .map(|&p| (p * SCALE as f64).round() as u32)
        .collect();

    let mut prefix_sum: u32 = quantized.iter().sum();
    while prefix_sum > SCALE {
        let (max_index, _) = quantized
            .iter()
            .enumerate()
            .max_by_key(|&(_, &v)| v)
            .expect("prefix_sum > SCALE >= 0 implies quantized is non-empty");
        quantized[max_index] -= 1;
        prefix_sum -= 1;
    }

    let mut result: Vec<u16> = quantized.into_iter().map(|v| v as u16).collect();
    result.push((SCALE - prefix_sum) as u16);
    result
}

/// Inverse of [`quantize`]: recovers approximate probabilities. Sums to exactly `1.0`
/// (since the inputs summed to exactly `SCALE`), unlike the original `f64` distribution,
/// which only summed to `1.0` within float tolerance.
pub fn dequantize(quantized: &[u16]) -> Vec<f64> {
    quantized.iter().map(|&v| v as f64 / SCALE as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::one_sided;

    #[test]
    fn quantize_of_a_single_value_distribution_is_exact() {
        assert_eq!(quantize(&[1.0]), vec![u16::MAX]);
    }

    #[test]
    fn quantize_then_dequantize_sums_to_exactly_one() {
        let distribution = [0.1, 0.2, 0.3, 0.4];
        let q = quantize(&distribution);
        let sum: u32 = q.iter().map(|&v| v as u32).sum();
        assert_eq!(sum, SCALE);

        let d = dequantize(&q);
        let d_sum: f64 = d.iter().sum();
        assert_eq!(d_sum, 1.0);
    }

    #[test]
    fn dequantize_is_close_to_the_original_distribution() {
        let distribution = [0.1, 0.2, 0.3, 0.4];
        let d = dequantize(&quantize(&distribution));
        for (i, (&original, &recovered)) in distribution.iter().zip(&d).enumerate() {
            assert!(
                (original - recovered).abs() < 1e-4,
                "index {i}: {original} vs {recovered}"
            );
        }
    }

    #[test]
    fn quantize_round_trips_every_position_in_the_one_sided_table() {
        // Exhaustive, not sampled: 54,264 finish distributions and 15,504 first_off
        // ones (see docs/rules-notes.md for why only off == 0 positions have first_off
        // data at all) is small enough to check every one, not a sample.
        let table = one_sided::compute_table();

        let mut max_finish_error = 0.0f64;
        let mut max_first_off_error = 0.0f64;
        let mut first_off_checked = 0usize;

        for entry in &table {
            let q = quantize(&entry.finish);
            let sum: u32 = q.iter().map(|&v| v as u32).sum();
            assert_eq!(sum, SCALE);
            let d = dequantize(&q);
            for (&original, &recovered) in entry.finish.iter().zip(&d) {
                max_finish_error = max_finish_error.max((original - recovered).abs());
            }

            if !entry.first_off.is_empty() {
                first_off_checked += 1;
                let q = quantize(&entry.first_off);
                let sum: u32 = q.iter().map(|&v| v as u32).sum();
                assert_eq!(sum, SCALE);
                let d = dequantize(&q);
                for (&original, &recovered) in entry.first_off.iter().zip(&d) {
                    max_first_off_error = max_first_off_error.max((original - recovered).abs());
                }
            }
        }

        assert_eq!(first_off_checked, 15_504);
        eprintln!(
            "max quantization error: finish {max_finish_error}, first_off {max_first_off_error}"
        );
        // Measured worst case across the full table is ~6.1e-5 (finish) and ~2.0e-5
        // (first_off) — a few units of SCALE, not one: the largest-value overshoot
        // correction can touch the same bucket more than once. 10 / SCALE is a loose
        // bound around that measurement, not a re-derivation of it from first
        // principles — this is a regression guard, and the eprintln above is the
        // actual number to look at.
        assert!(max_finish_error < 10.0 / SCALE as f64);
        assert!(max_first_off_error < 10.0 / SCALE as f64);
    }
}
