# Sweep

**Your desktop, swept. Every move is reversible.**

Sweep is a local-first desktop organizer for the folder you never open. It scans your Desktop and Downloads, classifies what is there, and proposes a plan that moves each item into one of five decay-based lanes. Nothing moves until you approve the plan. Every move is a transaction with a preconditions check, a write-ahead journal, and a verified undo. Vaults, keys, and repos are held back.

No cloud. No account. No network code. Offline is the architecture.

<img src="docs/mascot.png" alt="Sweep mascot" width="260" />

*Mascot generated with Grok Imagine.*

## The idea

The desktop is not a filing cabinet. It is a queue of undecided files. The expensive question is "what is this?" The filesystem already answers the cheap, reliable question: "when did I last touch it?"

Sweep organizes by decay, not by topic:

| Lane | Holds | Review |
|---|---|---|
| Bench | What you are using this week | weekly |
| Kits | Tools you authored and will run again | monthly |
| Shelf | Things you keep but are not using | monthly |
| Cold | Not needed now, kept for the record | quarterly |
| Inbox | Anything new, unknown, or "decide later" | weekly |

Every suggestion carries its evidence: age, kind, size, whether it is a git repo, whether it has a remote. No model. No guessing. A deterministic policy over filesystem facts.

**Bench stays in place.** The global plan moves Kits, Shelf, and high-confidence Cold items. It does not move Bench or Inbox; active and undecided files stay where they are. Use the lane button if you explicitly want to move them.

## What it does

```text
Scan -> Board -> Plan -> Refuse -> Apply -> Undo
```

- **Scan** reads your registered roots and indexes metadata into SQLite. Read-only.
- **Board** shows five lanes with counts and the top suggestions in each.
- **Plan** writes a versioned JSON file of proposed moves. Previewable, diffable, agent-generatable.
- **Refuse** holds back vaults, credentials, repos, and other risky items before a plan exists.
- **Apply** validates every op, writes a `pending` journal row, moves the file, then marks it `done`.
- **Undo** reverses the last applied plan in LIFO order and verifies each step.
- **Recover** reconciles an interrupted plan on the next start.

## Roots

Sweep does not assume a single folder. Add or remove any folder from the roots bar in the app. Desktop and Downloads are offered as one-click defaults, resolved through `dirs::desktop_dir()` and `dirs::download_dir()`: the Windows Known Folder API and XDG user directories on Linux. Each user's install resolves their own paths.

The CLI can choose roots at setup time:

```bash
sweep init --root "C:\Users\me\Desktop" --root "C:\dev\scripts"
```

The workspace (the five lane folders) defaults to `~/Sweep`; the SQLite index lives under the user's local app data. Both are per-user and created on first run.

## The safety model

Every scanned item is classified **safe**, **review**, or **blocked**. Only safe items can enter a plan. Review and blocked items are returned with the plan and shown in the preview, with the reason.

The engine enforces these invariants:

1. **No operation outside a registered root.** Paths are canonicalized and checked before execution.
2. **No overwrite.** Destinations are deduped at plan time with a ` (2)` suffix. Apply can suffix a file that appears between preview and execution (`--suffix`; the app passes it).
3. **Journal before mutation.** The op row is committed as `pending` before the filesystem is touched, then flipped to `done`.
4. **Undo is LIFO and targets the last applied plan.** A newer preview cannot hijack Undo; refuse if the file moved again.
5. **No deletion.** Sweep only moves. There is no trash command and no delete path.
6. **Dry run is the default and has no filesystem side effects.** `apply` without `--apply` validates and moves nothing.

## Refuses

| Class | Examples | Behavior |
|---|---|---|
| **Blocked** | Obsidian and Cryptomator vaults, Google Drive/Dropbox/iCloud sync roots, SSH keys, `gitaccess*`, `*.pem`, `*.ppk`, reparse points | Never in a plan; shown with the reason |
| **Review** | Git repositories, Godot projects, portable apps, ambiguous `*key*` names | Never in a plan; shown with the reason |
| **Safe** | Loose files, installers, archives, scripts, shortcuts, plain folders | Eligible for a plan |

`Review` is where a human belongs. Today the app holds these items and tells you why. Eventually Sweep can ask — a confirm-and-move prompt per item, still journaled and still undoable. v1.0 does not move them.

The core has unit and integration tests for the classifier, collision handling, and the apply-to-undo round trip. The crash harness covers recovery.

## Measurements

| Claim | Result |
|---|---|
| Scan the real desktop + Downloads | **73 + 38 items in 12 ms** |
| Crash harness | **500 files, 50 random kills, 50/50 clean recoveries, 0 lost files** |
| Duplicates after recovery | 0 |
| Zero-byte or partial files | 0 |
| Network dependencies | none |

The crash harness builds a 500-file tree, generates a 500-op plan, kills the process at 50 random points mid-apply, runs `sweep recover`, and asserts every file exists exactly once at its source or destination. The result is in [`docs/crash-harness.txt`](docs/crash-harness.txt); the harness is [`scripts/crash-harness.ps1`](scripts/crash-harness.ps1).

## Offline proof

- `cargo tree` contains no `reqwest`, `hyper`, `ureq`, or `tauri-plugin-http`.
- The Tauri capability manifest grants `core:default` and `dialog:default` (the folder picker only); the file system is reached through the Rust commands, not a network or fs plugin.
- The CSP allows only the local Tauri IPC channel and local assets. No external origins.
- The demo video is recorded in airplane mode.

## Architecture

```text
sweep-desktop/
  crates/
    sweep-core/   # indexer, classifier, policy, planner, journal, recovery, SQLite
    sweep-cli/    # init, scan, board, plan, apply, undo, recover
  app/            # Tauri 2 + Svelte 5 board
```

The core is a Rust library with no UI. The CLI is the engine's test harness and a scriptable surface. The app is a thin shell over the same commands.

### Data model

`workspace`, `root`, `item`, `zone`, `plan`, `op`. SQLite runs in WAL mode with `synchronous=FULL`. `item.safety` carries the classifier verdict; `op.state` is the safety story: `pending -> done -> undone`, or `failed`.

## Run it

**Requirements:** Rust (MSVC toolchain on Windows), Node 20+, npm, WebView2 (Windows).

```bash
cargo build --release
./target/release/sweep init
./target/release/sweep scan
./target/release/sweep board
./target/release/sweep plan --lane shelf --out plan.json
./target/release/sweep apply plan.json --apply
./target/release/sweep undo
./target/release/sweep recover
```

**App:**

```bash
cd app
npm install
npm run tauri dev
```

Launch the app from an interactive desktop session. WebView2 will not create a window from a service or SSH session (it fails with `Invalid window handle`).

## What is deliberately missing

- Freeform canvas; the lanes are the v1 surface.
- File watching; scan is manual.
- Native Windows icon rendering; the app is a board, not a desktop replacement.
- **`.lnk` target resolution.** v1 uses a name heuristic: a shortcut whose name contains `.ps1`, `.py`, `.sh`, `.bat`, `.cmd`, `.js`, or `.ts` suggests Kits. A shortcut named "StartWorkspace.ps1 - Shortcut.lnk" lands with the script it points at. True target resolution is v1.1.
- Duplicate detection. v1.1.
- **A confirm-and-move prompt for review items.** v1.0 holds them; eventually Sweep can ask.
- Tags, sync, cloud, accounts, themes, auto-update.
- Cross-volume moves. v1 is same-volume only.
- Review queue, drag-between-lanes, JSON rule engine, FTS5 search, archive manifests. These are v1.1; the hackathon build is the demo spine.

## What I would do next

1. Resolve `.lnk` targets and classify by what the shortcut points at.
2. Duplicate hints for Downloads: same name + size first, then hash.
3. A review queue that asks one question at a time, with the evidence attached.
4. Agent-generated plans: the plan file is already a stable JSON contract.
5. A table view for sorting by age, size, and kind.

## Why Rust

The core is a filesystem engine with crash-consistency requirements. Rust gives safe path handling, a real CLI, a small binary, and a test harness that can kill the process mid-move and prove recovery.

## Model declaration

- **Code and build:** Claude Opus 5.5 via Claude Code (architecture, review); DeepSeek V4.1 via Hermes Agent (implementation, tests).
- **Mascot art:** Grok Imagine (xAI).
- **Runtime:** no model. Sweep is deterministic and offline.

## License

MIT.
