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

/// Runs GNUbg's own move generator for `position` + `roll`, returning the
/// set of resulting positions it considers legal.
fn gnubg_resulting_positions(position: &Position, roll: Roll) -> HashSet<Position> {
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
