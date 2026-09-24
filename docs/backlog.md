# Backlog

Ideas that surfaced during work on the current phase but belong to a later phase. Not
prioritized; not commitments. See `OPENGAMMON.md` §4 for the phase each item likely belongs to.

- **Phase 1, done:** `og-core` now builds for `wasm32-unknown-unknown` (zero dependencies, as
  required by `CLAUDE.md` §4) and CI checks it on every push (`wasm` job in
  `.github/workflows/ci.yml`, `cargo build --target wasm32-unknown-unknown -p og-core`). Picked
  up ahead of Phase 2 rather than waiting for Phase 7: `og-bearoff` is about to introduce mmap
  file access, which doesn't work the same way under WASM (no filesystem in the browser), and
  this check needs to already be green before that lands so any WASM-incompatible code in
  `og-bearoff` is caught immediately, not discovered late in Phase 7. Extend the same job to
  `og-bearoff` once that crate exists.

- **Phase 1, resolved:** the `gnubg-cli.exe` memory-growth hypothesis for the earlier
  "gnubg-cli exited before answering a request" crashes (two attempts, both dying at
  ~2000–3000 positions with empty stderr) is ruled out. A clean, isolated 10k run — no
  concurrent `cargo` invocations — completed all 210,000 (position, roll) comparisons
  (10,000 positions × 21 rolls) in 4308s with zero mismatches and no crash. The actual cause of
  the earlier crashes was a concurrent `cargo build`/`cargo test` replacing the test binary
  mid-run (exit 127), already covered by the no-concurrent-`cargo` rule in `CLAUDE.md` §3.
  Session recycling for `GnubgSession` is dropped from the plan — it would have solved a
  problem that doesn't exist.

- **Phase 1, unresolved:** `cargo test -p og-core --lib` (debug profile, default parallelism)
  once crashed the whole test process with exit `0xffffffff` while
  `self_play::tests::phase_distribution_of_the_diff_test_sample` was running alongside the rest
  of the suite; the same test passes cleanly run in isolation. Not reproduced again since (the
  attempt was stopped rather than spending more time chasing it — see session history).

  Ruled out: the move generator's recursion depth. `collect_plies` was flagged as a possible
  WASM stack-safety risk (deep recursion on a dense, doubles-heavy position, worse on WASM's
  smaller stack) — checked and refuted, see `docs/rules-notes.md`: depth is bounded by dice
  count (proven and measured at exactly 5 for doubles), independent of branching factor. Not the
  cause here, and not a WASM risk to revisit — `og-core` already builds clean for
  `wasm32-unknown-unknown` (see the Phase 1 "done" entry above).

  Leading hypothesis instead: `phase_distribution_of_the_diff_test_sample` is an analysis, not
  an assertion-bearing test — it plays 10,000 self-play games (up to 120 turns each) inside one
  test thread, in an unoptimized debug build, concurrently with however many other test threads
  cargo's default parallelism spawns. That's plausibly enough combined CPU/memory pressure to
  have caused whatever aborted the process. Mitigated for now by marking that test `#[ignore]`
  (run explicitly, same as the GNUbg differential tests, ideally with `--release`). Not
  confirmed as *the* cause — if `cargo test --workspace` ever aborts like this again with that
  test excluded, this hypothesis is wrong and needs revisiting.

- **Phase 3:** `og-bearoff::disk::BearoffData::finish`/`first_off` allocate a `Vec<f64>` per call
  (decode the quantized record into a fresh, owned, dequantized vector every lookup). Fine for
  Phase 2's own validation and for the ~891ns-average measurement (`disk.rs`'s
  `finish_lookup_is_under_a_microsecond`), but rollouts (Phase 3) call bearoff lookup from the
  innermost loop, many times per trial, many trials per decision — that allocation pattern is
  exactly the kind of per-call heap traffic `CLAUDE.md` §4's "hot path has no allocations where
  avoidable" rule is about. Offer a zero-allocation access variant when Phase 3 actually needs it:
  either return the raw quantized `&[u16]` slice straight into the mmapped (or in-memory, for WASM)
  bytes and let the caller dequantize only the values it uses, or take a caller-supplied output
  buffer (`&mut [f64]`) to write into instead of allocating one. Not done now — no rollout code
  exists yet to benchmark against, and `CLAUDE.md` §4 is explicit: no optimization without a
  benchmark that's shown the problem first.
