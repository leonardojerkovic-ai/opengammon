"""Differential-test harness: asks a running GNU Backgammon (via its embedded
Python interpreter) for its own one-sided bearoff database index for a
checker placement, so a Rust caller can then look that index up with
`bearoffdump.exe` without spawning `gnubg-cli` once per position.

Long-lived, request/response protocol, same shape as `og-core`'s own
`gnubg_harness.py`: invoked once as `gnubg-cli -q -p
gnubg_bearoff_harness.py`, answers the opening-roll prompt, prints a READY
marker (see that script's module doc for why the marker matters -- GNUbg's
C core and Python's stdin both read fd 0), then reads one request per line
from stdin until EOF, writing one response per request to stdout.

Request, one line of 6 space-separated integers: the checker count on
points 1..=6 (index 0 = point 1), summing to at most 15. The remaining
checkers (15 - sum) are implicitly already off.

Response: one line, the integer bearoff index `gnubg.positionbearoff`
returns for that placement.
"""

import sys

import gnubg

READY_MARKER = "===OG_BEAROFF_HARNESS_READY==="


def handle_request(line):
    counts = [int(x) for x in line.split()]
    side = [0] * 25
    for i, count in enumerate(counts):
        side[i] = count
    index = gnubg.positionbearoff(tuple(side))
    print(index)
    sys.stdout.flush()


def main():
    gnubg.command("set player 0 human")
    gnubg.command("set player 1 human")
    gnubg.command("new session")  # consumes the "1 2" answer already queued on stdin

    # See og-core's gnubg_harness.py: the "new session" dice-roll prompt is read at
    # the C level, independently of Python's stdin buffering. This marker is the
    # handshake that tells the Rust side it's now safe to write requests.
    print(READY_MARKER)
    sys.stdout.flush()

    for line in sys.stdin:
        line = line.strip()
        if line:
            handle_request(line)


main()
