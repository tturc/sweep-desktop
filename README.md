# Sweep

**Your desktop, swept. Every move is reversible.**

Sweep is a local-first desktop organizer for the folder you never open. It scans your Desktop and Downloads, classifies what is there, and proposes a plan that moves each item into one of five decay-based lanes. Nothing moves until you approve the plan. Every move is a transaction with a preconditions check, a write-ahead journal, and a verified undo.

No cloud. No account. No network code. Offline is the architecture.

![Sweep board](docs/board.png)

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

## What it does

```text
Scan -> Board -> Plan -> Apply -> Undo
```

- **Scan** reads your registered roots and indexes metadata into SQLite. Read-only.
- **Board** shows five lanes with counts and the top suggestions in each.
- **Plan** writes a versioned JSON file of proposed moves. Previewable, diffable, agent-generatable.
- **Apply** validates every op, writes a `pending` journal row, moves the file, then marks it `done`.
- **Undo** reverses the plan in LIFO order and verifies each step.
- **Recover** reconciles an interrupted plan on the next start.

## Roots

Sweep does not assume a single folder. Add or remove any folder from the roots bar in the app. Desktop and Downloads are offered as one-click defaults, resolved through `dirs::desktop_dir()` and `dirs::download_dir()`: the Windows Known Folder API and XDG user directories on Linux. Each user's install resolves their own paths.

The CLI can choose roots at setup time:

```bash
sweep init --root "C:\Users\me\Desktop" --root "C:\dev\scripts"
```

The workspace (the five lane folders) defaults to `~/Sweep`; the SQLite index lives under the user's local app data. Both are per-user and created on first run.

## The safety model

Six invariants, each with a test:

1. **No operation outside a registered root.** Paths are canonicalized and checked at plan time and again before execution.
2. **No overwrite.** A destination collision aborts by default; `--suffix` appends ` (2)` and records the actual destination.
3. **Journal before mutation.** The op row is committed as `pending` before the filesystem is touched, then flipped to `done`.
4. **Undo is LIFO and verified.** Reverse order. Refuse if the file moved again.
5. **Trash is a quarantine folder.** Nothing is deleted. `sweep empty-trash` sends it to the OS bin.
6. **Dry run is the default.** `apply` without `--apply` validates and moves nothing.

## Measurements

| Claim | Result |
|---|---|
| Scan the real desktop + Downloads | **73 + 38 items in 12 ms** |
| Crash harness | **500 files, 50 random kills, 50/50 clean recoveries, 0 lost files** |
| Duplicates after recovery | 0 |
| Zero-byte or partial files | 0 |
| Network dependencies | none |

The crash harness builds a 500-file tree, generates a 500-op plan, kills the process at 50 random points mid-apply, runs `sweep recover`, and asserts every file exists exactly once at its source or destination. The result is in [`docs/crash-harness.txt`](docs/crash-harness.txt).

## Offline proof

- `cargo tree` contains no `reqwest`, `hyper`, `ureq`, or `tauri-plugin-http`.
- The Tauri capability manifest grants only `core:default`; the file system is reached through the Rust commands, not a network or fs plugin.
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

`workspace`, `root`, `item`, `zone`, `plan`, `op`. SQLite runs in WAL mode with `synchronous=FULL`. The `op.state` field is the safety story: `pending -> done -> undone`, or `failed`.

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

## What is deliberately missing

- Freeform canvas; the lanes are the v1 surface.
- File watching; scan is manual.
- Native Windows icon rendering; the app is a board, not a desktop replacement.
- **`.lnk` target resolution.** v1 uses a name heuristic: a shortcut whose name contains `.ps1`, `.py`, `.sh`, `.bat`, `.cmd`, `.js`, or `.ts` suggests Kits. A shortcut named "StartWorkspace.ps1 - Shortcut.lnk" lands with the script it points at. True target resolution is v1.1.
- Duplicate detection. v1.1.
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

## License

MIT.
