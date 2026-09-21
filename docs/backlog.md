# Backlog

Ideas that surfaced during work on the current phase but belong to a later phase. Not
prioritized; not commitments. See `OPENGAMMON.md` §4 for the phase each item likely belongs to.

- **Phase 0 / ongoing:** `og-core` (and eventually `og-bearoff`) must build for
  `wasm32-unknown-unknown`. Add a CI check (`cargo build --target wasm32-unknown-unknown -p
  og-core`) once `og-core` has real code — no point checking an empty crate. This guards the
  "near-zero dependencies, clean WASM" constraint from `CLAUDE.md` §4 before it can silently
  break in Phase 7.

- **Phase 1, unresolved:** `cargo test -p og-core --lib` (debug profile, default parallelism)
  once crashed the whole test process with exit `0xffffffff` while
  `self_play::tests::phase_distribution_of_the_diff_test_sample` was running alongside the rest
  of the suite; the same test passes cleanly run in isolation. Not reproduced again since (the
  attempt was stopped rather than spending more time chasing it — see session history).

  Ruled out: the move generator's recursion depth. `collect_plies` was flagged as a possible
  WASM stack-safety risk (deep recursion on a dense, doubles-heavy position, worse on WASM's
  smaller stack) — checked and refuted, see `docs/rules-notes.md`: depth is bounded by dice
  count (proven and measured at exactly 5 for doubles), independent of branching factor. Not the
  cause here, and not a WASM risk to revisit when the `wasm32-unknown-unknown` CI check above
  gets added.

  Leading hypothesis instead: `phase_distribution_of_the_diff_test_sample` is an analysis, not
  an assertion-bearing test — it plays 10,000 self-play games (up to 120 turns each) inside one
  test thread, in an unoptimized debug build, concurrently with however many other test threads
  cargo's default parallelism spawns. That's plausibly enough combined CPU/memory pressure to
  have caused whatever aborted the process. Mitigated for now by marking that test `#[ignore]`
  (run explicitly, same as the GNUbg differential tests, ideally with `--release`). Not
  confirmed as *the* cause — if `cargo test --workspace` ever aborts like this again with that
  test excluded, this hypothesis is wrong and needs revisiting.
