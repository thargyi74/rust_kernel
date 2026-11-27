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
- M2: done (all 3 parts built and reviewed, closed on 2026-11-27) and pushed. The ledger is `.superpowers/sdd/2026-11-25-m2-memory/progress.md` (git-ignored; its rulings and minors are also in `docs/study/02-memory.md`, "What really happened").
- M3 step 1, Study: done on 2026-11-27. The note is `docs/study/03-exceptions.md`. Its 5 open questions are at the end; question 4 is the Redox note's question 1.
- The user answered the 5 open questions on 2026-11-27: option A for all five. The answers are recorded in the note.
- M3 step 2, Plan: written on 2026-11-27. The plan is `docs/superpowers/plans/2026-11-27-m3-exceptions.md`: 11 tasks in 3 parts. The ledger will be `.superpowers/sdd/2026-11-27-m3-exceptions/progress.md` (git-ignored). Its Decisions 1–11 are "proposed by Claude"; Decision 1 (the stack-overflow check, Task 3) differs from the note. The Redox note's question 2 is for M4.
- **Next: M3 step 3, Build Part 1**, once the user has reviewed the plan. Start text: `Run Part 1 of docs/superpowers/plans/2026-11-27-m3-exceptions.md (Tasks 1–4), following its "Execution in 3 parts" section.`

**Redox side step, outside the milestone cycle — done on 2026-11-27.** The release comparison and its two open questions are in [the Redox overview](docs/study/redox-overview.md). The prerequisite for the M3 study is complete.
- **When:** after M2 closes, before the M3 study (the user chose this on 2026-11-27). The M2 Close session should end by reminding the user to start the Redox session next. If a session is asked for the M3 study while this side step is still open, it should remind the user first.
- **Goal:** one study note, `docs/study/redox-overview.md`. It compares the Redox OS kernel (a Rust microkernel, MIT license) with Linux and with our design. Use `docs/study/TEMPLATE.md` where it fits.
- **Source:** clone `https://gitlab.redox-os.org/redox-os/kernel` into `reference/redox` and pin it to a release tag. `reference/` is git-ignored; never commit it. Read the code, not the website: `doc.redox-os.org` refused automated reads (HTTP 403).
- **Topics:**
  - what runs in the kernel and what runs in user space ("schemes");
  - the ARM64 boot;
  - memory: page tables, the frame allocator and the heap;
  - how `unsafe` code is kept in one place;
  - task switching;
  - system-call entry.
- **For each topic:** say how Linux does it, how we do it (or plan to), and whether we should change anything. Cite `file:line` in `reference/redox` and check every citation.
- **Rules:** we learn from Redox; we don't copy its code. Quote only short excerpts. Our "no crates" rule still holds, even though Redox uses crates.
- **Finish:** commit the note, mark this side step done here, and end by asking the user the note's open questions.
