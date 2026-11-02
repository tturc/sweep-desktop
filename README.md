# Sweep

A local-first desktop organizer that treats file moves as transactions. No cloud, no account, no network code. Offline is the architecture.

## Status

Hackyard Yard #6 build, 2-6 Nov 2026. Demo spine: scan, board, plan, apply, undo, recover.

## Commands

```
sweep init
sweep scan
sweep board
sweep plan --lane kits --out plan.json
sweep apply plan.json --apply
sweep undo
sweep recover
```

Every mutating command is dry-run by default. `apply` without `--apply` validates the plan and moves nothing.
