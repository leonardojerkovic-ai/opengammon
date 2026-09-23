//! Differential tests against a real GNU Backgammon, spawned as an external process
//! (see CLAUDE.md — GNUbg is never a dependency, only ever invoked from tests).
//! `#[ignore]`d, requires `GNUBG_PATH` to point at `gnubg-cli(.exe)`. Run explicitly:
//!
//! ```text
//! cargo test -p og-bearoff -- --ignored
//! ```
//!
//! Two GNUbg tools are used, both exactly as GNUbg ships them (no source read or
//! copied): `gnubg.positionbearoff()`, through the same embedded-Python layer
//! `og-core`'s own `gnubg_harness.py` uses (see `tests/gnubg_bearoff_harness.py`), maps
//! a checker placement to GNUbg's own bearoff index; `bearoffdump.exe`, a CLI tool
//! shipped alongside `gnubg-cli`, dumps that index's rolls-needed distribution from
//! `gnubg_os0.bd` as human-readable text. See `docs/rules-notes.md` for what was learned
//! getting here: GNUbg's own index convention differs from `combinatorial::rank`'s, and
//! its "saving gammon" statistic (used here as `first_off`) is retroactive — trivial and
//! not comparable for any position with `off > 0`.

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::combinatorial;
use crate::one_sided::{self, Entry, POINTS};

const READY_MARKER: &str = "===OG_BEAROFF_HARNESS_READY===";

fn harness_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("gnubg_bearoff_harness.py")
}

fn gnubg_exe() -> PathBuf {
    PathBuf::from(
        env::var("GNUBG_PATH")
            .expect("GNUBG_PATH must point at gnubg-cli(.exe) to run GNUbg differential tests"),
    )
}

fn gnubg_dir() -> PathBuf {
    gnubg_exe()
        .parent()
        .expect("GNUBG_PATH has a parent directory")
        .to_path_buf()
}

fn bearoffdump_exe() -> PathBuf {
    gnubg_dir().join("bearoffdump.exe")
}

fn bearoff_db() -> PathBuf {
    gnubg_dir().join("gnubg_os0.bd")
}

/// A single long-lived `gnubg-cli` process answering `gnubg.positionbearoff()` queries,
/// one checker placement per line in, one GNUbg bearoff index out. Reused across many
/// queries instead of paying process-startup cost per query (same reasoning as
/// `og-core`'s `GnubgSession`).
struct BearoffIndexSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl BearoffIndexSession {
    fn spawn() -> Self {
        let mut child = Command::new(gnubg_exe())
            .arg("-q")
            .arg("-p")
            .arg(harness_script())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn gnubg-cli; check GNUBG_PATH");

        let mut stdin = child.stdin.take().expect("child stdin was piped");
        stdin
            .write_all(b"1 2\n")
            .expect("failed to write to gnubg-cli stdin");

        let mut stdout = BufReader::new(child.stdout.take().expect("child stdout was piped"));
        // Block until the harness's READY marker: writing anything before this risks
        // GNUbg's C-level stdin read for the opening prompt swallowing it (see
        // og-core's gnubg_harness.py module doc for the full explanation).
        loop {
            let mut line = String::new();
            let n = stdout
                .read_line(&mut line)
                .expect("failed to read gnubg-cli stdout while waiting for READY");
            assert!(n > 0, "gnubg-cli exited before printing READY");
            if line.trim_end() == READY_MARKER {
                break;
            }
        }

        BearoffIndexSession {
            child,
            stdin,
            stdout,
        }
    }

    fn index(&mut self, checkers: [u8; POINTS]) -> u32 {
        let request: String = checkers
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(self.stdin, "{request}").expect("failed to write request to gnubg-cli");

        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .expect("failed to read gnubg-cli stdout");
        assert!(
            n > 0,
            "gnubg-cli exited before answering a bearoff-index request"
        );
        line.trim()
            .parse()
            .unwrap_or_else(|_| panic!("gnubg-cli returned a non-integer bearoff index: {line:?}"))
    }
}

impl Drop for BearoffIndexSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// GNUbg's own rolls-needed distributions for one position, read from `bearoffdump.exe`'s
/// text output. Percentages (0..100), not probabilities, matching the tool's own units.
struct GnubgDump {
    /// "Bearing off, Opponent" column: `finish_pct[r]` for roll count `r = 0..`.
    finish_pct: Vec<f64>,
    /// "Bearing at least one chequer off, Opponent" column (GNUbg's "saving gammon"):
    /// only meaningful when the queried position has `off == 0` — see the module doc.
    /// `first_off_pct[r]` for roll count `r = 0..`.
    first_off_pct: Vec<f64>,
}

fn parse_gnubg_number(field: &str) -> f64 {
    field
        .trim()
        .replace(',', ".")
        .parse()
        .unwrap_or_else(|_| panic!("not a GNUbg-formatted number: {field:?}"))
}

fn dump_bearoff(index: u32) -> GnubgDump {
    let output = Command::new(bearoffdump_exe())
        .arg("-n")
        .arg(index.to_string())
        .arg(bearoff_db())
        .output()
        .expect("failed to run bearoffdump.exe; check GNUBG_PATH's directory");
    assert!(
        output.status.success(),
        "bearoffdump.exe exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);

    let mut finish_pct = Vec::new();
    let mut first_off_pct = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        if line.trim_start().starts_with("Rolls") && line.contains("Player") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let Some(roll_field) = fields.first() else {
            break;
        };
        if roll_field.trim().parse::<u32>().is_err() {
            break; // end of the data rows (e.g. the blank line before "Average rolls")
        }
        finish_pct.push(fields.get(2).map_or(0.0, |f| parse_gnubg_number(f)));
        first_off_pct.push(fields.get(5).map_or(0.0, |f| parse_gnubg_number(f)));
    }

    GnubgDump {
        finish_pct,
        first_off_pct,
    }
}

/// Largest absolute difference, in percentage points, between our own distribution
/// (converted from probabilities) and GNUbg's, padding the shorter one with zeros.
///
/// `gnubg_row_offset` aligns index conventions: `ours[r]` is compared against
/// `gnubg_pct[r + gnubg_row_offset]`. 0 for `finish` (both mean "r rolls" directly);
/// 1 for `first_off` (`ours[i]` means "i + 1 rolls", but GNUbg's dump rows are 0-indexed
/// directly by roll count — see the call site).
fn max_deviation_pct(ours: &[f64], gnubg_pct: &[f64], gnubg_row_offset: usize) -> f64 {
    (0..ours.len().max(gnubg_pct.len()))
        .map(|r| {
            let our_pct = ours.get(r).copied().unwrap_or(0.0) * 100.0;
            let their_pct = gnubg_pct.get(r + gnubg_row_offset).copied().unwrap_or(0.0);
            (our_pct - their_pct).abs()
        })
        .fold(0.0, f64::max)
}

/// Result of comparing one position's `og-bearoff` entry against GNUbg's own values.
struct Comparison {
    checkers: [u8; POINTS],
    finish_deviation_pct: f64,
    first_off_deviation_pct: Option<f64>,
}

fn compare_position(
    checkers: [u8; POINTS],
    table: &[Entry],
    session: &mut BearoffIndexSession,
) -> Comparison {
    let ours = one_sided::lookup(table, checkers);
    let gnubg_index = session.index(checkers);
    let dump = dump_bearoff(gnubg_index);

    let total: u32 = checkers.iter().map(|&c| c as u32).sum();
    let off_zero = total == one_sided::MAX_CHECKERS as u32;

    Comparison {
        checkers,
        finish_deviation_pct: max_deviation_pct(&ours.finish, &dump.finish_pct, 0),
        first_off_deviation_pct: if off_zero {
            // GNUbg's row 0 is always 0.000 for an off == 0 position (you can't have
            // already saved gammon with zero rolls elapsed) and has no counterpart in
            // ours, which never represents "0 rolls" at all: shift by one.
            Some(max_deviation_pct(&ours.first_off, &dump.first_off_pct, 1))
        } else {
            None
        },
    }
}

/// Same as [`compare_position`], but compares the *quantized* (`quantize::quantize`
/// then `dequantize`) values instead of the raw `f64` table — the values `og-bearoff`
/// actually stores and would serve from a lookup, not an intermediate the DP happens to
/// compute along the way. Quantization error is tiny (see `quantize.rs`'s own
/// exhaustive test) but not zero, so this can legitimately differ slightly from
/// `compare_position`'s result on the same position.
fn compare_position_quantized(
    checkers: [u8; POINTS],
    table: &[Entry],
    session: &mut BearoffIndexSession,
) -> Comparison {
    let ours = one_sided::lookup(table, checkers);
    let gnubg_index = session.index(checkers);
    let dump = dump_bearoff(gnubg_index);

    let total: u32 = checkers.iter().map(|&c| c as u32).sum();
    let off_zero = total == one_sided::MAX_CHECKERS as u32;

    let quantized_finish = crate::quantize::dequantize(&crate::quantize::quantize(&ours.finish));

    Comparison {
        checkers,
        finish_deviation_pct: max_deviation_pct(&quantized_finish, &dump.finish_pct, 0),
        first_off_deviation_pct: if off_zero {
            let quantized_first_off =
                crate::quantize::dequantize(&crate::quantize::quantize(&ours.first_off));
            Some(max_deviation_pct(
                &quantized_first_off,
                &dump.first_off_pct,
                1,
            ))
        } else {
            None
        },
    }
}

/// A tolerance for GNUbg's own 3-decimal-percentage display rounding (max 0.0005 per
/// value) plus a margin for small accumulated float differences near a distribution's
/// peak, where many summed roll/recursion paths converge (observed up to ~0.012 on a
/// 300-position sample; this stays an order of magnitude above that, well below the
/// scale of a real mismatch like the first_off policy bug, which was ~47 points off).
const TOLERANCE_PCT: f64 = 0.05;

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};

    use super::*;

    /// 10 handpicked positions spanning: few/many checkers on board, stacked/spread
    /// across points, close/far from finishing, and (deliberately) several with some
    /// checkers already off — see docs/rules-notes.md for why `first_off` is only
    /// checked at `off == 0`. Run before the larger random sample: if this fails, the
    /// cause is more findable in 10 positions than in a few hundred.
    #[test]
    #[ignore = "requires GNUBG_PATH"]
    fn ten_diverse_positions_match_gnubg() {
        let table = one_sided::compute_table();
        let mut session = BearoffIndexSession::spawn();

        let positions: [[u8; POINTS]; 10] = [
            [0, 0, 0, 0, 0, 15], // off=0, stacked, far
            [3, 2, 3, 2, 3, 2],  // off=0, spread, moderate
            [15, 0, 0, 0, 0, 0], // off=0, stacked, close
            [5, 4, 3, 2, 1, 0],  // off=0, spread, close-ish
            [1, 1, 1, 4, 4, 4],  // off=0, mixed
            [0, 0, 0, 0, 0, 5],  // off=10, few, stacked, far
            [1, 1, 1, 1, 1, 0],  // off=10, few, spread, near
            [0, 0, 0, 0, 0, 1],  // off=14, very few, far
            [1, 0, 0, 0, 0, 0],  // off=14, very few, close
            [2, 2, 2, 2, 2, 3],  // off=2, many-ish, spread
        ];

        let mut worst_finish = 0.0f64;
        let mut worst_first_off = 0.0f64;
        for checkers in positions {
            let result = compare_position(checkers, &table, &mut session);
            worst_finish = worst_finish.max(result.finish_deviation_pct);
            if let Some(d) = result.first_off_deviation_pct {
                worst_first_off = worst_first_off.max(d);
            }
            assert!(
                result.finish_deviation_pct < TOLERANCE_PCT,
                "finish mismatch at {:?}: {} pct points",
                result.checkers,
                result.finish_deviation_pct
            );
            if let Some(d) = result.first_off_deviation_pct {
                assert!(
                    d < TOLERANCE_PCT,
                    "first_off mismatch at {:?}: {} pct points",
                    result.checkers,
                    d
                );
            }
        }

        eprintln!(
            "10 positions: largest finish deviation {worst_finish} pct points, \
             largest first_off deviation {worst_first_off} pct points"
        );
    }

    fn random_checkers(rng: &mut StdRng) -> [u8; POINTS] {
        // Uniform total (1..=15) then a uniformly random point per checker: naturally
        // produces a mix of few/many, stacked/spread, close/far positions without
        // needing to hand-tune the distribution.
        let total = rng.random_range(1..=one_sided::MAX_CHECKERS as u32);
        let mut checkers = [0u8; POINTS];
        for _ in 0..total {
            checkers[rng.random_range(0..POINTS)] += 1;
        }
        checkers
    }

    /// Scaled-up version of `ten_diverse_positions_match_gnubg`: Phase 2's "done"
    /// criterion is a match on a sample, not one position. Fixed seed, same tolerance.
    #[test]
    #[ignore = "requires GNUBG_PATH"]
    fn a_few_hundred_random_positions_match_gnubg() {
        const SAMPLE_SIZE: u32 = 300;
        const SEED: u64 = 0xb0ad1ce;

        let table = one_sided::compute_table();
        let mut session = BearoffIndexSession::spawn();
        let mut rng = StdRng::seed_from_u64(SEED);

        let mut worst_finish = 0.0f64;
        let mut worst_first_off = 0.0f64;
        let mut off_zero_checked = 0u32;

        for _ in 0..SAMPLE_SIZE {
            let checkers = random_checkers(&mut rng);
            let result = compare_position(checkers, &table, &mut session);

            worst_finish = worst_finish.max(result.finish_deviation_pct);
            assert!(
                result.finish_deviation_pct < TOLERANCE_PCT,
                "finish mismatch at {:?}: {} pct points",
                result.checkers,
                result.finish_deviation_pct
            );

            if let Some(d) = result.first_off_deviation_pct {
                off_zero_checked += 1;
                worst_first_off = worst_first_off.max(d);
                assert!(
                    d < TOLERANCE_PCT,
                    "first_off mismatch at {:?}: {} pct points",
                    result.checkers,
                    d
                );
            }
        }

        eprintln!(
            "{SAMPLE_SIZE} random positions ({off_zero_checked} with off == 0 checked for \
             first_off): largest finish deviation {worst_finish} pct points, largest \
             first_off deviation {worst_first_off} pct points"
        );
    }

    /// Exhaustive, not sampled: every one of the 54,264 one-sided positions, comparing
    /// the *quantized* values (`compare_position_quantized`) against GNUbg — Phase 2's
    /// actual "done" criterion is a match against GNUbg, and the values that need to
    /// match are the ones `og-bearoff` would really serve, not an f64 intermediate.
    /// Slow (~55 minutes sequential over one `bearoffdump.exe` call per position, per
    /// CLAUDE.md rules-notes.md's discussion) — run explicitly, not part of the regular
    /// `-- --ignored` sweep of this module's other tests.
    #[test]
    #[ignore = "requires GNUBG_PATH; slow, ~55 minutes -- run explicitly"]
    fn exhaustive_quantized_comparison_matches_gnubg() {
        let table = one_sided::compute_table();
        let mut session = BearoffIndexSession::spawn();

        let total_positions = table.len();
        let mut worst_finish = 0.0f64;
        let mut worst_first_off = 0.0f64;
        let mut off_zero_checked = 0u32;

        for index in 0..total_positions {
            let checkers: [u8; POINTS] = combinatorial::unrank(one_sided::MAX_CHECKERS, index);
            if checkers == [0; POINTS] {
                // bearoffdump.exe's `-n 0` is rejected as "index not provided" (a CLI
                // quirk in the tool itself, confirmed manually). The only position this
                // ever affects is all-off, where finish == [1.0] is a structural base
                // case anyway (already checked exhaustively in one_sided.rs), not
                // something a GNUbg comparison would add confidence to. Skipped, not
                // silently passed: this is the one index this test cannot reach.
                continue;
            }
            let result = compare_position_quantized(checkers, &table, &mut session);

            worst_finish = worst_finish.max(result.finish_deviation_pct);
            assert!(
                result.finish_deviation_pct < TOLERANCE_PCT,
                "finish mismatch at index {index} {:?}: {} pct points",
                result.checkers,
                result.finish_deviation_pct
            );

            if let Some(d) = result.first_off_deviation_pct {
                off_zero_checked += 1;
                worst_first_off = worst_first_off.max(d);
                assert!(
                    d < TOLERANCE_PCT,
                    "first_off mismatch at index {index} {:?}: {} pct points",
                    result.checkers,
                    d
                );
            }

            if index % 2000 == 0 {
                eprintln!(
                    "exhaustive_quantized_comparison_matches_gnubg: {index}/{total_positions} \
                     ({off_zero_checked} off==0 checked), worst so far: finish \
                     {worst_finish} pct points, first_off {worst_first_off} pct points"
                );
            }
        }

        assert_eq!(off_zero_checked, 15_504);
        eprintln!(
            "all {total_positions} positions ({off_zero_checked} with off == 0 checked for \
             first_off): largest finish deviation {worst_finish} pct points, largest \
             first_off deviation {worst_first_off} pct points"
        );
    }
}
