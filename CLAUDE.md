# rust_kernel: notes for Claude

**The project.** This is a learning project: study Linux v7.3 and rebuild it as a Linux-like kernel in pure Rust.
- The Linux source is in `reference/linux`. It is pinned to v7.3 and git-ignored.
- ARM64 comes first (M0–M7), then x86-64 (M8) and RISC-V (M9).
- The roadmap is in `docs/superpowers/plans/2026-11-25-rust-kernel.md`.

## Fixed rules

**Code**
- All `unsafe` code lives in `foundation`. Every other crate has `#![forbid(unsafe_code)]`.
- No crates.io crates, only `core` and `alloc`. `cargo xtask test` checks this.
- The license is GPL-2.0-only. Every `.rs` file starts with the SPDX line and names its Linux counterpart.
- Run `. "$HOME/.cargo/env"` first in every shell.

**Git**
- Never edit or commit anything under `reference/`.
- Commit straight to `main`.
- Push to `origin main` only when the user asks. Never force-push.
- `docs/` (study notes and plans) is tracked. Commit it along with the work it describes.

**Working with the user**
- The user is on the Pro plan. Do **one step per fresh session** (see below), and never chain steps in one session.
- The user's first language is not English. Use short, plain sentences. Give options, with one recommendation.

## The milestone cycle

Every milestone goes through the steps below. Each step is its own fresh session. The user starts it with:

    Continue rust_kernel: do the next step (see CLAUDE.md).

**In each session:**
1. Read **Status** to find the step.
2. Do only that step.
3. Update Status and commit.
4. Stop.

**The steps:**
1. **Study.**
   - Read the Linux files in the milestone's roadmap row.
   - Write `docs/study/NN-<topic>.md` from `docs/study/TEMPLATE.md`, where NN is the milestone number in two digits.
   - Check every `file:line` citation against `reference/linux`.
   - End by asking the user the note's open questions.
2. **Plan.** Do this once the user has answered.
   - Use superpowers:writing-plans to write `docs/superpowers/plans/<date>-mN-<topic>.md`. Base it on the code as it is now.
   - Split it into parts of 3–5 tasks, with an "Execution in parts" section like the M2 plan's.
   - Give a rough token-cost estimate. For scale, M1 took about 26M cached re-read tokens, 0.34M new input and 0.18M output.
   - Ask the user to review the plan.
3. **Build, one part per session.** Follow the plan's "Execution in parts" rules: superpowers:executing-plans, Native, with a fresh Opus reviewer at the end of each part.
4. **Close.** Do this at the end of the last part's session.
   - Add "what really happened" to the study note.
   - Mark the milestone done in the roadmap.
   - Update `README.md`.
   - Ask whether to push.

## Status

- M0 and M1: done and pushed.
- **M2: plan accepted. Next: Part 1 (Tasks 1–5)** of `docs/superpowers/plans/2026-11-25-m2-memory.md`. Follow its "Execution in 3 parts" section.
