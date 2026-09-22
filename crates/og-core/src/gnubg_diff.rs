//! Differential tests against a real GNU Backgammon, spawned as an external
//! process (see CLAUDE.md — GNUbg is never a dependency, only ever invoked
//! from tests). Each test is `#[ignore]`d and requires `GNUBG_PATH` to point
//! at `gnubg-cli(.exe)`. Run explicitly:
//!
//! ```text
//! cargo test -p og-core -- --ignored
//! ```
//!
//! GNUbg's move generator is queried through its embedded Python layer
//! (`tests/gnubg_harness.py`), not by parsing its ASCII board — see that
//! script's module doc for the exact wire format. One [`GnubgSession`] is a
//! single long-lived gnubg-cli process answering many queries in turn, so a
//! run over many positions doesn't pay process-startup cost per query.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::{CheckerMove, Destination, Die, Origin, Ply, PointIndex, Position, Roll};

const RESULT_MARKER: &str = "===OG_HARNESS_RESULT===";
const END_MARKER: &str = "===OG_HARNESS_END===";
const READY_MARKER: &str = "===OG_HARNESS_READY===";

/// The harness's default MAX_MOVES cap, used by every differential test
/// except the one that specifically probes whether that cap is high enough.
const DEFAULT_MAX_MOVES: u32 = 5000;

fn harness_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("gnubg_harness.py")
}

fn gnubg_exe() -> String {
    env::var("GNUBG_PATH")
        .expect("GNUBG_PATH must point at gnubg-cli(.exe) to run GNUbg differential tests")
}

/// A single long-lived `gnubg-cli` process, driven through the request/response
/// protocol in `gnubg_harness.py`. Reused across many `resulting_positions`
/// calls instead of spawning one process per query.
struct GnubgSession {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
    stderr_tail: Arc<Mutex<Vec<String>>>,
}

impl GnubgSession {
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
        // Answers the opening-roll prompt `new session` always asks for
        // inside the harness script; the position it produces is
        // immediately overwritten by the first real request.
        stdin
            .write_all(b"1 2\n")
            .expect("failed to write to gnubg-cli stdin");

        let stdout = BufReader::new(child.stdout.take().expect("child stdout was piped"));

        // Drained on a dedicated thread rather than read at the end: a
        // child that blocks writing to a full, unread stderr pipe would
        // otherwise deadlock against us blocking on its stdout.
        let stderr = BufReader::new(child.stderr.take().expect("child stderr was piped"));
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        let stderr_tail_writer = Arc::clone(&stderr_tail);
        thread::spawn(move || {
            for line in stderr.lines().map_while(Result::ok) {
                let mut tail = stderr_tail_writer.lock().unwrap();
                tail.push(line);
                let len = tail.len();
                if len > 200 {
                    tail.drain(0..len - 200);
                }
            }
        });

        let mut session = GnubgSession {
            child,
            stdin: Some(stdin),
            stdout,
            stderr_tail,
        };

        // Block until the harness script confirms it has moved past
        // GNUbg's own C-level read of the "1 2" handshake and is now the
        // sole reader of stdin (see the READY_MARKER comment in
        // gnubg_harness.py) -- writing a request any earlier risks it being
        // silently swallowed by GNUbg's internal buffering instead of
        // reaching Python, deadlocking both sides.
        let mut line = String::new();
        loop {
            line.clear();
            let n = session
                .stdout
                .read_line(&mut line)
                .expect("failed to read from gnubg-cli stdout");
            assert!(
                n > 0,
                "gnubg-cli exited before signaling readiness\nstderr tail:\n{}",
                session.stderr_snapshot()
            );
            if line.trim_end() == READY_MARKER {
                break;
            }
        }

        session
    }

    fn stderr_snapshot(&self) -> String {
        self.stderr_tail.lock().unwrap().join("\n")
    }

    /// Runs GNUbg's own move generator for `position` + `roll`, returning
    /// the set of resulting positions it considers legal.
    fn resulting_positions(
        &mut self,
        position: &Position,
        roll: Roll,
        max_moves: u32,
    ) -> HashSet<Position> {
        let bar = position.bar();
        let (d1, d2) = roll.dice();
        let points = (0..24)
            .map(|i| position.point(i).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let request = format!(
            "{points} {} {} {} {} {max_moves}\n",
            bar[0],
            bar[1],
            d1.get(),
            d2.get()
        );

        let stdin = self.stdin.as_mut().expect("session stdin already closed");
        stdin
            .write_all(request.as_bytes())
            .expect("failed to write to gnubg-cli stdin");
        stdin.flush().expect("failed to flush gnubg-cli stdin");

        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .expect("failed to read from gnubg-cli stdout");
            assert!(
                n > 0,
                "gnubg-cli exited before answering a request\nstderr tail:\n{}",
                self.stderr_snapshot()
            );
            if line.trim_end() == RESULT_MARKER {
                break;
            }
        }

        let mut result = HashSet::new();
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .expect("failed to read from gnubg-cli stdout");
            assert!(
                n > 0,
                "gnubg-cli exited mid-response\nstderr tail:\n{}",
                self.stderr_snapshot()
            );
            let trimmed = line.trim();
            if trimmed == END_MARKER {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            let ply = trimmed
                .split_whitespace()
                .map(parse_sub_move)
                .fold(Ply::empty(), Ply::pushed);
            result.insert(position.apply(&ply));
        }
        result
    }
}

impl Drop for GnubgSession {
    fn drop(&mut self) {
        // Close stdin (send EOF) so the harness's request loop exits and the
        // interpreter shuts down cleanly, then reap the process.
        self.stdin.take();
        let _ = self.child.wait();
    }
}

/// Parses a `"from,to"` token in GNUbg's own 1..24/0(off)/25(bar) numbering
/// (`gnubg.parsemove`'s convention) into an og-core `CheckerMove`.
fn parse_sub_move(token: &str) -> CheckerMove {
    let (from, to) = token.split_once(',').expect("malformed sub-move token");
    let from: u8 = from.parse().expect("non-integer 'from'");
    let to: u8 = to.parse().expect("non-integer 'to'");
    CheckerMove {
        from: if from == 25 {
            Origin::Bar
        } else {
            Origin::Point(PointIndex::new(from - 1))
        },
        to: if to == 0 {
            Destination::Off
        } else {
            Destination::Point(PointIndex::new(to - 1))
        },
    }
}

fn our_resulting_positions(position: &Position, roll: Roll) -> HashSet<Position> {
    position
        .generate_moves(roll)
        .into_iter()
        .map(|ply| position.apply(&ply))
        .collect()
}

fn assert_matches_gnubg(session: &mut GnubgSession, position: &Position, roll: Roll) {
    let ours = our_resulting_positions(position, roll);
    let theirs = session.resulting_positions(position, roll, DEFAULT_MAX_MOVES);
    assert_eq!(
        ours,
        theirs,
        "\nposition: {position:?}\nroll: {roll:?}\nonly ours: {:?}\nonly gnubg's: {:?}",
        ours.difference(&theirs).collect::<Vec<_>>(),
        theirs.difference(&ours).collect::<Vec<_>>(),
    );
}

/// Splits `n` items into `workers` contiguous, roughly-equal, non-overlapping
/// `[start, end)` ranges (the first `n % workers` ranges get one extra item),
/// covering `0..n` with no gaps.
fn chunk_bounds(n: usize, workers: usize) -> Vec<(usize, usize)> {
    let base = n / workers;
    let remainder = n % workers;
    let mut start = 0;
    (0..workers)
        .map(|w| {
            let size = base + usize::from(w < remainder);
            let end = start + size;
            let bounds = (start, end);
            start = end;
            bounds
        })
        .collect()
}

#[test]
fn chunk_bounds_partitions_without_gaps_or_overlap() {
    for n in [0usize, 1, 20, 21, 1_000_000] {
        for workers in 1..=16usize {
            let bounds = chunk_bounds(n, workers);
            assert_eq!(bounds.len(), workers);
            assert_eq!(bounds[0].0, 0, "n={n} workers={workers}");
            assert_eq!(bounds.last().unwrap().1, n, "n={n} workers={workers}");
            for pair in bounds.windows(2) {
                assert_eq!(
                    pair[0].1, pair[1].0,
                    "gap or overlap between chunks: n={n} workers={workers} bounds={bounds:?}"
                );
            }
            for &(start, end) in &bounds {
                assert!(start <= end, "n={n} workers={workers} bounds={bounds:?}");
            }
            let total: usize = bounds.iter().map(|&(s, e)| e - s).sum();
            assert_eq!(total, n, "n={n} workers={workers}");
        }
    }
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn starting_position_matches_gnubg() {
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(
        &mut session,
        &Position::starting(),
        Roll::new(Die::new(1), Die::new(3)),
    );
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn entry_from_bar_matches_gnubg() {
    let mut points = [0i8; 24];
    points[18] = -2;
    let position = Position::from_raw(points, [1, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(3), Die::new(3)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn entry_from_bar_blocked_matches_gnubg() {
    let mut points = [0i8; 24];
    points[18..24].fill(-2);
    points[12] = 1;
    let position = Position::from_raw(points, [1, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(2), Die::new(5)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn forced_higher_die_when_only_one_playable_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[3] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(3), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn doubles_with_fewer_than_four_moves_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[0] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(4), Die::new(4)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn bearoff_requires_all_checkers_home_matches_gnubg() {
    let mut points = [0i8; 24];
    points[5] = 1;
    points[12] = 1;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(6), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn bearoff_from_higher_die_matches_gnubg() {
    let mut points = [0i8; 24];
    points[2] = 1;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(6), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn no_legal_moves_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[10] = -2;
    points[9] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    let mut session = GnubgSession::spawn();
    assert_matches_gnubg(&mut session, &position, Roll::new(Die::new(2), Die::new(3)));
}

/// The 8 tests above are all sparse, synthetic positions (a handful of
/// checkers, hand-placed to isolate one rule). CLAUDE.md §0 flags that
/// MAX_MOVES=5000 has never been checked against dense, realistic
/// (15-checkers-a-side) positions, where branching is highest. Confirms the
/// cap doesn't silently drop moves by comparing it against a cap 40x larger
/// on self-play-generated positions where nobody has borne off yet.
#[test]
#[ignore = "requires GNUBG_PATH"]
fn dense_positions_are_not_truncated_by_max_moves_cap() {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use crate::self_play::random_reachable_position;

    let mut rng = StdRng::seed_from_u64(2024);
    let turn_counts = [4u32, 8, 15];
    let mut session = GnubgSession::spawn();

    for &turns in &turn_counts {
        for _ in 0..10 {
            let position = random_reachable_position(&mut rng, turns);
            assert_eq!(
                position.off(),
                [0, 0],
                "sample should still have all 30 checkers in play"
            );

            for d1 in 1..=6u8 {
                for d2 in d1..=6u8 {
                    let roll = Roll::new(Die::new(d1), Die::new(d2));
                    let capped = session.resulting_positions(&position, roll, DEFAULT_MAX_MOVES);
                    let uncapped = session.resulting_positions(&position, roll, 200_000);
                    assert_eq!(
                        capped,
                        uncapped,
                        "MAX_MOVES={DEFAULT_MAX_MOVES} silently truncated the legal-move list\nposition: {position:?}\nroll: {roll:?}\nmissing: {:?}",
                        uncapped.difference(&capped).collect::<Vec<_>>(),
                    );
                }
            }
        }
    }
}

/// The Phase 1 "done" bar (CLAUDE.md §0): a million random-but-reachable
/// positions x all 21 rolls, identical legal-move set to GNUbg. Run
/// explicitly and only on request — sequentially this test runs for on the
/// order of 100 hours (measured ~17ms/query at 0-ply x 21M queries), so it
/// parallelizes across `OG_DIFF_WORKERS` independent `GnubgSession`s (each
/// its own `gnubg-cli` process), one per contiguous, disjoint slice of the
/// sample.
///
/// - `OG_DIFF_SAMPLE_SIZE` overrides the position count (e.g. for the
///   smaller checkpoint runs on the way to a million); default 1_000_000.
/// - `OG_DIFF_WORKERS` sets how many `GnubgSession`s run concurrently;
///   default 1 (sequential, the original behavior). Pick this close to the
///   machine's core count for an overnight run — each worker is one
///   `gnubg-cli` process plus this test's own (cheap) move generation.
/// - `OG_DIFF_CHECKPOINT_DIR` overrides where per-worker progress is
///   recorded; default `target/gnubg_diff_checkpoint` at the workspace
///   root (already gitignored). Each worker writes the next global sample
///   index it has yet to start to `worker_<NN>.txt` after every position it
///   finishes, so re-running the *same* command after an interruption
///   (crash, reboot, Ctrl-C) resumes every worker from its own checkpoint
///   instead of redoing already-verified positions. The checkpoint layout
///   is keyed to `OG_DIFF_WORKERS`: changing the worker count between runs
///   makes old checkpoints line up with different ranges, so delete
///   `OG_DIFF_CHECKPOINT_DIR` first if you do that (or if you want a clean
///   re-run for any other reason).
///
/// A mismatch in any worker stops all of them (checked once per position,
/// not per roll) rather than letting the others burn hours of GNUbg queries
/// after the run has already failed; the first mismatch found is what gets
/// reported, with the same paste-able position/roll detail as the other
/// differential tests.
#[test]
#[ignore = "requires GNUBG_PATH; slow -- see module doc"]
fn random_self_play_positions_match_gnubg() {
    use crate::self_play::diff_test_sample;

    let sample_size: usize = env::var("OG_DIFF_SAMPLE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);

    let workers: usize = env::var("OG_DIFF_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&w| w > 0)
        .unwrap_or(1);

    let checkpoint_dir: PathBuf = env::var("OG_DIFF_CHECKPOINT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("target")
                .join("gnubg_diff_checkpoint")
        });
    fs::create_dir_all(&checkpoint_dir).expect("failed to create checkpoint directory");

    let all_rolls: Vec<Roll> = (1..=6u8)
        .flat_map(|d1| (d1..=6u8).map(move |d2| Roll::new(Die::new(d1), Die::new(d2))))
        .collect();
    assert_eq!(all_rolls.len(), 21);

    // `diff_test_sample` is the one source of truth for this sample's seed
    // and per-position turn-count draw: any other analysis of "the" Phase 1
    // sample (e.g. its phase breakdown in self_play.rs) reads it from there
    // too, and stays in sync with this test by construction rather than by
    // convention. Materialized once up front rather than sliced lazily per
    // worker: it's pure self-play with no GNUbg calls (cheap), and doing it
    // once avoids every worker but the first re-walking the shared RNG from
    // zero just to reach its own starting index.
    let positions: Vec<Position> = diff_test_sample().take(sample_size).collect();
    assert_eq!(positions.len(), sample_size);

    let bounds = chunk_bounds(sample_size, workers);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let stop = AtomicBool::new(false);

    thread::scope(|scope| {
        for (worker_id, &(start, end)) in bounds.iter().enumerate() {
            let positions = &positions;
            let all_rolls = &all_rolls;
            let failure = &failure;
            let stop = &stop;
            let checkpoint_path = checkpoint_dir.join(format!("worker_{worker_id:02}.txt"));

            scope.spawn(move || {
                let resume_from = fs::read_to_string(&checkpoint_path)
                    .ok()
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .filter(|&n| n >= start && n <= end)
                    .unwrap_or(start);

                if resume_from >= end {
                    // Already finished by an earlier run of this same
                    // worker range.
                    return;
                }

                let mut session = GnubgSession::spawn();

                let range = positions
                    .iter()
                    .enumerate()
                    .skip(resume_from)
                    .take(end - resume_from);

                for (i, position) in range {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }

                    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                        for &roll in all_rolls {
                            assert_matches_gnubg(&mut session, position, roll);
                        }
                    }));

                    if let Err(payload) = outcome {
                        let message = payload
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                            .unwrap_or_else(|| "gnubg differential mismatch".to_string());
                        let mut guard = failure.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(format!("worker {worker_id}, sample index {i}: {message}"));
                        }
                        drop(guard);
                        stop.store(true, Ordering::Relaxed);
                        return;
                    }

                    fs::write(&checkpoint_path, (i + 1).to_string())
                        .expect("failed to write checkpoint file");

                    if (i - start) % 1000 == 0 {
                        eprintln!(
                            "random_self_play_positions_match_gnubg: worker {worker_id} at {i}/{end} (sample {sample_size} total)"
                        );
                    }
                }
            });
        }
    });

    if let Some(message) = failure.into_inner().unwrap() {
        panic!("{message}");
    }
}
