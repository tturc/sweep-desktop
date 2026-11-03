# Refusal Layer — v1.0 Build Plan

**Decision (2026-11-03): Option B.** Keep the five-lane sorter and its proven transaction engine. Add the safety/refusal layer, fix the demo-level bugs, and ship. Places, the rules table, anchors, and migration move to v1.1.

**Deadline:** Hackyard Yard #6 submission Wed 4 Nov 2026, 12:30 PM EST (Speedrun cutoff 1:00 PM).

## What ships

1. **Undo targets the last applied plan**, not the newest draft preview.
2. **Dry run is side-effect free** — validation never creates folders.
3. **Collisions are resolved at plan time**, with ` (2)` suffixes; apply also suffixes a late collision.
4. **Every item is classified** `safe` / `review` / `blocked` at scan time.
5. **No plan ever contains a review or blocked item.** They are returned in the plan's `held` list with a reason and shown in the preview.
6. **The board and preview show refusals** — a lock badge on held items and grouped Ready / Review / Manual sections.
7. **The README tells the truth** — no test claim it cannot back, no missing screenshot, no `empty-trash` feature that does not exist, and the `dialog:default` capability is documented.

## What is deliberately cut

- Places as filing destinations (Work / Projects / Resources / Utils / Archive / Inbox).
- A stored rules table and matcher engine. A Rust `match` is enough for v1.
- First-run migration and anchors (`Resources`, `Utils`).
- Collections/tags.
- A confirm-and-move button for review items.

**Future (v1.1):** review items are held out in v1.0. Eventually Sweep can ask — a per-item "move this anyway?" confirmation with the reason attached, then the same journaled move. That is a UX layer on the existing transaction engine, not a new safety model.

## Safety classes

### Blocked — never in any plan

- Reparse points: junctions, symlinks, cloud placeholders (`FILE_ATTRIBUTE_REPARSE_POINT` on Windows).
- Obsidian vaults: top-level `.obsidian`, or a direct child that contains `.obsidian`.
- Cryptomator: `masterkey.cryptomator`, `vault.cryptomator`, `*.cryptomator`.
- Cloud sync roots: `.tmp.driveupload`, `.tmp.drivedownload`, `.dropbox`, `.icloud`.
- SSH/credential material: `id_rsa*`, `id_ed25519*`, `*.pem`, `*.ppk`, `*.p12`, `*.pfx`, `*.kdbx`, `gitaccess*`, `masterkey*`, `*secret*`.
- A folder whose direct children contain blocked material (e.g. `Keys/`).

### Review — held out, shown with a reason

- Git repositories (`.git`).
- Godot projects (`project.godot` at top level or one level down).
- Portable apps (top-level `.exe` plus `.ini`).
- Names that only look sensitive (`*key*`, e.g. "Keyboard shortcuts.pdf"). Held, not blocked, because the match is broad.

### Safe — eligible for a plan

Everything else. A plain folder moves whole; its children are not planned separately.

## Plan rules

- Only top-level items (one path component) become ops. Nested files move with their folder; they are never planned twice.
- A plan's `ops` contains only safe items. `held` contains every review/blocked item the plan considered, with `name`, `path`, `safety`, `lane`, and `reason`.
- Destination collisions are deduped inside the plan (`name (2).ext`) before preview, and apply is called with `suffix=true` to handle a file that appears between plan and apply.
- A plan with zero safe ops but a non-empty `held` list is valid: the UI opens the preview to show what was refused.

## Files

- `crates/sweep-core/src/model.rs` — `Safety`, `Item.safety`, `Item.safety_reason`.
- `crates/sweep-core/src/safety.rs` — classifier + unit tests.
- `crates/sweep-core/src/walk.rs` — classify each scanned item.
- `crates/sweep-core/src/db.rs` — guarded `ALTER TABLE item`, `last_applied_plan()`, tests.
- `crates/sweep-core/src/plan.rs` — `held`, top-level filter, collision dedupe, safety gate.
- `crates/sweep-core/src/apply.rs` — side-effect-free validation, late-collision suffix, apply→undo test.
- `crates/sweep-core/src/undo.rs` — use `last_applied_plan()`.
- `crates/sweep-cli/src/main.rs` — allow a held-only plan; print held counts.
- `app/src-tauri/src/lib.rs` — safety on `SuggestionView`; `suffix=true`; allow held-only plans.
- `app/src/routes/+page.svelte` — lock badges, Ready / Review / Manual preview, no 10-op truncation.
- `README.md` — truth pass + "eventually it could ask".

## Test plan

- Safety fixtures: `id_rsa`, `notes.txt`, `.obsidian` folder, `.git` repo, `.exe` + `.ini`, `Keyboard shortcuts.pdf`.
- DB: `last_applied_plan()` ignores a newer draft.
- Apply → undo round trip in a temp root, proving the undo targets the applied plan.
- Plan: collision dedupe, blocked/review never in `ops`, held-only plan is valid.
- Windows: `cargo test`, `cargo build --release`, `npm run check`, `npm run tauri build`.
- Real desktop dry-run after a desktop copy; confirm all protected items are held.

## Demo plan (Tom's call)

1. **Copy the desktop first** (robocopy or zip to a dated folder outside the desktop). At minimum, copy every `src` in the plan to `Desktop-sweep-backup-<date>`.
2. Record the scan and preview on the **actual desktop**.
3. Apply the safe plan; show a blocked item (`Obsidian_Vault`) and a review item (`x-growth-engine`) in the held sections.
4. Undo; the desktop returns.
5. Airplane mode visible; no network code.

The staged root remains the fallback if the real desktop looks too messy or a copy fails.

## Verified on this build (2026-11-03)

- `cargo test`: 14 tests green on Windows, including the apply-to-undo round trip, the draft-preview undo regression, side-effect-free dry run, collision dedupe, and the safety fixtures.
- Real desktop audit: **24 safe ops, 21 held**. Blocked: `NoteVault`, `Obsidian`, `Obsidian_Vault`, `Keys`, `gitaccess.txt`, `hermes-osint-ctf-agent`. Review: the repos, `Godot`, `mp3val`, `snes`, `Utils`, `GameDevProjects`, `VaderShare`.
- Dry run on the real desktop: `24 ops validated` with no files moved.
- Crash harness re-run: **50/50 clean recoveries**, 0 lost, 0 duplicates.
- `npm run check`: 0 errors. `npm run build`: green. `npm run tauri build`: MSI and NSIS bundles produced.

