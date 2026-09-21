"""Differential-test harness: asks a running GNU Backgammon (via its embedded
Python interpreter) for the legal plies at one position + roll, machine-readably
(GNUbg's own move generator/notation, not a parse of its ASCII board).

Invoked as `gnubg-cli -q -p gnubg_harness.py`, fed "1 2\n" on stdin to answer
GNUbg's opening-roll prompt (an artifact of needing *a* game in progress
before `set board` is accepted; the position it produces is immediately
overwritten). Deliberately no JSON on either side of this script, to avoid
pulling a JSON dependency into og-core's Rust side for what's otherwise a
handful of integers.

Input, via the OG_HARNESS_INPUT environment variable (GNUbg's CLI arg parser
treats trailing positional args as .sgf files to load, so argv isn't usable
here): one line of 28 space-separated integers —
    <points[0..24]> <bar_mine> <bar_opponent> <die1> <die2>
using og-core's own convention (point index 0..23 = point 1..24 from the
perspective of the player on roll; positive = that player's checkers,
negative = the opponent's).

Output: a line containing exactly RESULT_MARKER, then one line per legal
ply, each a space-separated list of "from,to" sub-moves using GNUbg's own
1..24/0(off)/25(bar) numbering (gnubg.parsemove's convention) — the Rust
side translates that into og-core's Origin/Destination types. Zero plies
(no legal moves) is an empty output after the marker.
"""

import os
import sys

import gnubg

RESULT_MARKER = "===OG_HARNESS_RESULT==="

# Generous upper bound: GNUbg generates the full legal-move list first and
# ranks/truncates only for display, but no realistic backgammon position
# comes close to this many distinct legal plies. Overridable via
# OG_HARNESS_MAX_MOVES so a test can compare the real cap against a much
# larger one and confirm it never actually truncates (see
# `dense_positions_are_not_truncated_by_max_moves_cap` in gnubg_diff.rs).
MAX_MOVES = int(os.environ.get("OG_HARNESS_MAX_MOVES", "5000"))


def main():
    fields = [int(x) for x in os.environ["OG_HARNESS_INPUT"].split()]
    points, bar, dice = fields[0:24], fields[24:26], fields[26:28]

    gnubg.command("set player 0 human")
    gnubg.command("set player 1 human")
    gnubg.command("new session")

    # Seat 1 is always the player on roll in this harness. Seat 0's board is
    # the opponent's checkers, expressed in the opponent's own point numbering
    # (point p in the caller's frame is point 25 - p in the opponent's frame).
    mover = [0] * 25
    opponent = [0] * 25
    for i, count in enumerate(points):
        if count > 0:
            mover[i] = count
        elif count < 0:
            opponent[23 - i] = -count
    mover[24] = bar[0]
    opponent[24] = bar[1]

    position_id = gnubg.positionid((opponent, mover))
    gnubg.command("set board " + position_id)
    gnubg.command("set turn 1")
    gnubg.command("set dice %d %d" % (dice[0], dice[1]))

    result = gnubg.hint(MAX_MOVES)

    print(RESULT_MARKER)
    for entry in result.get("hint", []):
        move_string = entry["move"]
        if move_string in ("(No moves)", "Cannot move"):
            continue
        pairs = gnubg.parsemove(move_string)
        print(" ".join("%d,%d" % (frm, to) for frm, to in pairs))


main()
