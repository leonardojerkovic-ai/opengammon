# Contributing

OpenGammon works phase by phase — see [`OPENGAMMON.md`](./OPENGAMMON.md) §4 for the current
phase and its definition of done, and [`CLAUDE.md`](./CLAUDE.md) for day-to-day rules (crate
boundaries, testing policy, commit style).

## Ground rules

- Never copy code from GNUbg (GPLv3) or eXtremeGammon into this repository, in whole or
  paraphrased. GNUbg may only be invoked as an external process, and only from tests and
  `og-eval`. Reading GNUbg source to understand a rule or an edge case is fine; porting its
  structures or functions is not.
- Never train on GNUbg or XG evaluations. Their rollouts are for validation only.
- `og-core` and `og-bearoff` are test-first: write the test before the implementation.
- Dependencies flow one way: `og-core` → `og-bearoff` → `og-engine` → `og-rollout`. Propose
  any new external dependency before adding it, especially in `og-core` and `og-bearoff`,
  which target zero or near-zero dependencies so they compile cleanly to WASM.

## Before opening a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same checks on every push, plus a separate scheduled job for `#[ignore]`d slow
tests (large-scale differential tests against GNUbg, million-position runs).

## Commit messages

`<crate>: short imperative summary`, e.g. `og-core: add bar entry move generation`. Use
`docs` or `ci` as the prefix for changes outside a single crate.
