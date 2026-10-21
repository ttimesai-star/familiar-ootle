<!-- Jules session 1284298185826579547, review of commit d1f30c4 (pre-submission polish) from a snapshot in the author's workspace repository. After this review two follow-ups, 7d5cecf and 955d41a: rustc 1.99.1 pinned for the template build, and the same doc comment rewrapped to two lines so the wasm hash stays 653ebb3f. -->

# Code Review: POLISH_DIFF.patch

Reviewer: Jules
Date: 2026-10-22
Target Snapshot: `projects/contests/tari_ootle/review_snapshot_familiar/`
Patch File: `projects/contests/tari_ootle/review_snapshot_familiar/POLISH_DIFF.patch`

## Summary of Verification Checks

1. **Template doc comment (`templates/familiar/src/lib.rs`)**:
   - The comment correctly states that the recall leaf uses a dedicated random recall key held by the owner (not the access rules owner key) and that sweeping requires co-signing with the recall key.
   - Matches implementation in `client/src/cli.rs` (`recall_keys`, `build_tx_cosigned` with `add_signer`) and description in `README.md`.

2. **`OWNER_FEES` test configuration (`client/src/cli.rs`)**:
   - `OWNER_FEES` is annotated with `#[cfg(test)]`.
   - Verified that `OWNER_FEES` is only referenced within `#[cfg(test)] mod tests` in `cli.rs` (specifically in `a_whole_leash_fits_in_one_take_from_the_tap`). No non-test references exist, so no build break occurs.

3. **`TARI_TOKEN` constant usage (`tests/tap.rs`)**:
   - `XTR` was replaced with `TARI_TOKEN` from `tari_template_lib::types::constants`.
   - `TARI_TOKEN` is the active, non-deprecated constant in `tari_template_lib` 0.34.

4. **CI Workflow (`.github/workflows/ci.yml`)**:
   - The `rustfmt (template, tap, workspace)` step is valid YAML.
   - Correct commands: `(cd templates/familiar && cargo fmt --check)`, `(cd templates/tap && cargo fmt --check)`, and `cargo fmt --all --check`.

5. **README and Documentation Claims / Links**:
   - Quickstart in `README.md` references release `esmeralda-2026-10-21-recall-key` and links to `[docs/releases.md](docs/releases.md)`.
   - `docs/releases.md` exists and lists release history accurately.
   - LLM run notes in `README.md` and `docs/llm_agent_run.md` consistently note that the LLM run pre-dates cycle 3.
   - The Familiar vs Ghostline section accurately explains the architectural differences.
   - All relative links resolve.

## Findings

no findings
