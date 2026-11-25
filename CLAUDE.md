# rust_kernel: notes for Claude

**The project.** This is a learning project: study Linux v7.3 and rebuild it as a Linux-like kernel in pure Rust.
- The Linux source is in `reference/linux`. It is pinned to v7.3 and git-ignored.
- ARM64 comes first (M0–M7), then x86-64 (M8) and RISC-V (M9).
- The roadmap is in `docs/superpowers/plans/2026-11-25-rust-kernel.md`.

## Fixed rules

**Code**
- All `unsafe` code lives in `foundation`. Every other crate has `#![forbid(unsafe_code)]`, except `vmlinux`, whose one-line entry glue needs `#[unsafe(no_mangle)]`.
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
   - Give a rough token-cost estimate. For scale, M0 and M1 together took about 26M cached re-read tokens, 0.33M new input and 0.17M output (M0 about 4M / 0.03M / 0.02M, M1 about 22M / 0.30M / 0.15M).
   - Ask the user to review the plan.
3. **Build, one part per session.** Follow the plan's "Execution in parts" rules: superpowers:executing-plans, Native, with a fresh Opus reviewer at the end of each part.
4. **Close.** Do this at the end of the last part's session.
   - Add "what really happened" to the study note.
   - Mark the milestone done in the roadmap.
   - Update `README.md`.
   - Ask whether to push.

## Status

- M0 and M1: done and pushed.
- M2 Part 1 (Tasks 1–5): done and reviewed, not pushed. The ledger is `.superpowers/sdd/2026-11-25-m2-memory/progress.md`.
- **M2: next is Part 2 (Tasks 6–8)** of `docs/superpowers/plans/2026-11-25-m2-memory.md`. Follow its "Execution in 3 parts" section, and read its Decision 7 first.
