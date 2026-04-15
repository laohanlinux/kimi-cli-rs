---
name: Session Recovery
description: Checkpointing and recovery strategy so translation work survives abnormal restarts
type: reference
---

**Checkpoint directory:** `/Users/rg/Projects/kimi-cli-rs/checkpoints/`

**Recovery mechanism:**
- After every major module group is translated, a checkpoint file is written with the current progress.
- On restart, read the latest checkpoint to determine the next unimplemented module.
- The `MEMORY.md` index always points to the current project context.

**Checkpoint format (`checkpoints/latest.json`):**
```json
{
  "phase": 4,
  "completed_modules": ["...all major modules..."],
  "in_progress_modules": [],
  "next_module_group": "Integration, tests, and detailed tool implementations",
  "last_updated": "2026-04-14T21:00:00Z",
  "notes": "All major module groups translated to compilable stubs. Both library and binary build successfully with cargo build."
}
```

**How to resume after restart:**
1. Read `checkpoints/latest.json`.
2. List `src/` to verify what already exists.
3. Continue from `next_module_group`.
4. Run `cargo check` to verify compilation baseline.

**Active task list (as of last update):**
- All tasks through #4 (Translate remaining module groups) are completed.
- #5 Integration and tests is pending.
