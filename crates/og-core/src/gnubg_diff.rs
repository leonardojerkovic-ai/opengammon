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
//! script's module doc for the exact wire format.

use std::collections::HashSet;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{CheckerMove, Destination, Die, Origin, Ply, PointIndex, Position, Roll};

const RESULT_MARKER: &str = "===OG_HARNESS_RESULT===";

fn harness_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("gnubg_harness.py")
}

fn gnubg_exe() -> String {
    env::var("GNUBG_PATH")
        .expect("GNUBG_PATH must point at gnubg-cli(.exe) to run GNUbg differential tests")
}

/// The harness's default MAX_MOVES cap (see `gnubg_harness.py`), used by
/// every differential test except the one that specifically probes whether
/// that cap is high enough.
const DEFAULT_MAX_MOVES: u32 = 5000;

/// Runs GNUbg's own move generator for `position` + `roll`, returning the
/// set of resulting positions it considers legal.
fn gnubg_resulting_positions(position: &Position, roll: Roll) -> HashSet<Position> {
    gnubg_resulting_positions_capped(position, roll, DEFAULT_MAX_MOVES)
}

/// Like [`gnubg_resulting_positions`], but with an explicit override for the
/// harness's MAX_MOVES cap, to check that cap doesn't silently truncate the
/// legal-move list on dense positions.
fn gnubg_resulting_positions_capped(
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
    let input = format!("{points} {} {} {} {}", bar[0], bar[1], d1.get(), d2.get());

    let mut child = Command::new(gnubg_exe())
        .arg("-q")
        .arg("-p")
        .arg(harness_script())
        .env("OG_HARNESS_INPUT", &input)
        .env("OG_HARNESS_MAX_MOVES", max_moves.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn gnubg-cli; check GNUBG_PATH");

    // Answers the opening-roll prompt `new session` always asks for inside
    // the harness script; the position it produces is immediately overwritten.
    child
        .stdin
        .take()
        .expect("child stdin was piped")
        .write_all(b"1 2\n")
        .expect("failed to write to gnubg-cli stdin");

    let output = child
        .wait_with_output()
        .expect("gnubg-cli did not exit cleanly");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let after_marker = stdout.find(RESULT_MARKER).unwrap_or_else(|| {
        panic!(
            "gnubg-cli produced no result marker.\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });

    stdout[after_marker + RESULT_MARKER.len()..]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let checker_moves = line.split_whitespace().map(parse_sub_move);
            let ply = checker_moves.fold(Ply::empty(), Ply::pushed);
            position.apply(&ply)
        })
        .collect()
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

fn assert_matches_gnubg(position: &Position, roll: Roll) {
    let ours = our_resulting_positions(position, roll);
    let theirs = gnubg_resulting_positions(position, roll);
    assert_eq!(
        ours,
        theirs,
        "\nposition: {position:?}\nroll: {roll:?}\nonly ours: {:?}\nonly gnubg's: {:?}",
        ours.difference(&theirs).collect::<Vec<_>>(),
        theirs.difference(&ours).collect::<Vec<_>>(),
    );
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn starting_position_matches_gnubg() {
    assert_matches_gnubg(&Position::starting(), Roll::new(Die::new(1), Die::new(3)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn entry_from_bar_matches_gnubg() {
    let mut points = [0i8; 24];
    points[18] = -2;
    let position = Position::from_raw(points, [1, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(3), Die::new(3)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn entry_from_bar_blocked_matches_gnubg() {
    let mut points = [0i8; 24];
    points[18..24].fill(-2);
    points[12] = 1;
    let position = Position::from_raw(points, [1, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(2), Die::new(5)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn forced_higher_die_when_only_one_playable_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[3] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(3), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn doubles_with_fewer_than_four_moves_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[0] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(4), Die::new(4)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn bearoff_requires_all_checkers_home_matches_gnubg() {
    let mut points = [0i8; 24];
    points[5] = 1;
    points[12] = 1;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(6), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn bearoff_from_higher_die_matches_gnubg() {
    let mut points = [0i8; 24];
    points[2] = 1;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(6), Die::new(6)));
}

#[test]
#[ignore = "requires GNUBG_PATH"]
fn no_legal_moves_matches_gnubg() {
    let mut points = [0i8; 24];
    points[12] = 1;
    points[10] = -2;
    points[9] = -2;
    let position = Position::from_raw(points, [0, 0], [0, 0]);
    assert_matches_gnubg(&position, Roll::new(Die::new(2), Die::new(3)));
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
                    let capped =
                        gnubg_resulting_positions_capped(&position, roll, DEFAULT_MAX_MOVES);
                    let uncapped = gnubg_resulting_positions_capped(&position, roll, 200_000);
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
