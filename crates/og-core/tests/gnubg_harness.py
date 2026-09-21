"""Differential-test harness: asks a running GNU Backgammon (via its embedded
Python interpreter) for the legal plies at one position + roll, machine-readably
(GNUbg's own move generator/notation, not a parse of its ASCII board).

Long-lived, request/response protocol: invoked once as `gnubg-cli -q -p
gnubg_harness.py`, it answers GNUbg's opening-roll prompt (an artifact of
needing *a* game in progress before `set board` is accepted; the position it
produces is immediately overwritten by the first real request), then reads
one request per line from stdin until EOF, writing one response per request
to stdout. This lets the Rust side reuse a single process across many
queries instead of paying process-startup cost per query -- the difference
between a differential run over a handful of positions and one over a
million (see CLAUDE.md §0). Deliberately no JSON on either side, to avoid
pulling a JSON dependency into og-core's Rust side for what's otherwise a
handful of integers.

Request, one line of 29 space-separated integers:
    <points[0..24]> <bar_mine> <bar_opponent> <die1> <die2> <max_moves>
using og-core's own convention (point index 0..23 = point 1..24 from the
perspective of the player on roll; positive = that player's checkers,
negative = the opponent's). `max_moves` is the cap passed to `gnubg.hint()`;
normally the harness's default (see `gnubg_diff.rs`), overridden only by the
test that checks the cap doesn't silently truncate the legal-move list.

Response: a line containing exactly RESULT_MARKER, then one line per legal
ply, each a space-separated list of "from,to" sub-moves using GNUbg's own
1..24/0(off)/25(bar) numbering (gnubg.parsemove's convention) -- the Rust
side translates that into og-core's Origin/Destination types -- then a line
containing exactly END_MARKER. Zero plies (no legal moves) is just the two
markers with nothing between them.
"""

import sys

import gnubg

RESULT_MARKER = "===OG_HARNESS_RESULT==="
END_MARKER = "===OG_HARNESS_END==="
READY_MARKER = "===OG_HARNESS_READY==="


def handle_request(line):
    fields = [int(x) for x in line.split()]
    points, bar, dice, max_moves = fields[0:24], fields[24:26], fields[26:28], fields[28]

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

    result = gnubg.hint(max_moves)

    print(RESULT_MARKER)
    for entry in result.get("hint", []):
        move_string = entry["move"]
        if move_string in ("(No moves)", "Cannot move"):
            continue
        pairs = gnubg.parsemove(move_string)
        print(" ".join("%d,%d" % (frm, to) for frm, to in pairs))
    print(END_MARKER)
    sys.stdout.flush()


def main():
    gnubg.command("set player 0 human")
    gnubg.command("set player 1 human")
    # hint() ranks candidates by equity, which we never look at -- we only
    # want the complete set of resulting positions. 0-ply chequerplay
    # evaluation is far cheaper and doesn't change which moves are legal or
    # how many hint() returns, only their order/scores.
    gnubg.command("set evaluation chequerplay evaluation plies 0")
    gnubg.command("new session")  # consumes the "1 2" answer already queued on stdin

    # GNUbg's own C core and Python's sys.stdin both read fd 0: the "new
    # session" dice-roll prompt above is read at the C level, buffered
    # independently of Python's io layer. If a real request is written to
    # stdin before that C-level read has definitely finished, it can be
    # silently swallowed into GNUbg's internal buffer instead of reaching
    # Python -- and then both sides block forever. This marker is the
    # handshake that tells the Rust side it's now safe to write: nothing
    # after this point reads stdin except this loop.
    print(READY_MARKER)
    sys.stdout.flush()

    for line in sys.stdin:
        line = line.strip()
        if line:
            handle_request(line)


main()
