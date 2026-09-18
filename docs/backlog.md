# Backlog

Ideas that surfaced during work on the current phase but belong to a later phase. Not
prioritized; not commitments. See `OPENGAMMON.md` §4 for the phase each item likely belongs to.

- **Phase 0 / ongoing:** `og-core` (and eventually `og-bearoff`) must build for
  `wasm32-unknown-unknown`. Add a CI check (`cargo build --target wasm32-unknown-unknown -p
  og-core`) once `og-core` has real code — no point checking an empty crate. This guards the
  "near-zero dependencies, clean WASM" constraint from `CLAUDE.md` §4 before it can silently
  break in Phase 7.
