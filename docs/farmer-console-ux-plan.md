# Farmer Console UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve `bevy_console` command discovery and completion for Farmer's in-game developer console.

**Architecture:** Keep public integration simple: Farmer continues to register `clap::Command` values through `ConsoleConfiguration.commands` and static completions through `arg_completions`. `bevy_console` owns command candidate computation, Tab acceptance/cycling, and popup rendering. No Farmer-specific command names or assumptions enter this crate.

**Tech Stack:** Rust, Bevy 0.18, bevy_egui 0.39, clap 4.5, existing trie-rs prefix search.

---

### Task 1: Candidate Search Model

**Files:**
- Modify: `src/console.rs`

- [ ] Add tests for prefix, substring, and fuzzy command ranking.
- [ ] Implement a small candidate ranking helper that combines existing trie prefix matches with command-name substring/fuzzy matches.
- [ ] Keep `arg_completions` in the candidate source.
- [ ] Run `rtk cargo test`.

### Task 2: Tab Completion Semantics

**Files:**
- Modify: `src/console.rs`

- [ ] Add tests for accepting a single candidate and cycling multiple candidates.
- [ ] Implement pure helpers for applying a candidate to the input buffer.
- [ ] Update UI key handling: Tab accepts single candidate; with multiple candidates it opens/highlights and cycles; Enter accepts highlighted candidate before command submit.
- [ ] Run `rtk cargo test`.

### Task 3: Suggestions Popup Styling

**Files:**
- Modify: `src/console.rs`

- [ ] Add configuration fields for suggestion popup background, border, selected row background, and max width behavior.
- [ ] Ensure `Clone` and `Default` preserve all new fields.
- [ ] Render suggestions inside a non-transparent `egui::Frame`.
- [ ] Run `rtk cargo test`.

### Task 4: Farmer Integration

**Files:**
- Modify: `D:/proj/Farmer_Worktrees/bevy-console-integration/application/Cargo.toml`
- Modify: `D:/proj/Farmer_Worktrees/bevy-console-integration/Cargo.lock`

- [ ] Change Farmer `bevy_console` dependency to `https://github.com/TeacherZeng/bevy-console`, branch `farmer/console-ux`.
- [ ] Run Farmer console tests and feature checks.
