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
  "phase": 11,
  "completed_modules": ["...all core modules and many tools..."],
  "in_progress_modules": [
    "src/soul/agent.rs post-init bindings",
    "src/wire/server.rs full JSON-RPC/WebSocket",
    "src/web/app.rs + auth.rs + models.rs + runner/* + store/*",
    "src/vis/ visualization pipeline",
    "src/ui/print/ + ui/shell/ rich features",
    "src/notifications/ full queue + delivery",
    "src/utils/ remaining utilities",
    "src/background/ persistence store",
    "src/acp/ full handlers",
    "src/plugin/ manager + tool",
    "src/cli/ full commands"
  ],
  "next_module_group": "Remaining large modules: wire/server.rs, web/*, vis/*, ui/shell/*, notifications/*, utils/*, background/store, acp/*, plugin/*, cli/*",
  "last_updated": "2026-04-16T14:30:00+08:00",
  "notes": "Completed approval system, message helpers, context improvements, toolset enhancements, KimiSoul hook events + auto-compaction + wire integration, slash command wire sends, background manager fleshing, AskUserQuestion wire protocol, EnterPlanMode/ExitPlanMode wire protocol, TaskList/TaskOutput/TaskStop parity, config JSON migration + Theme enum, session assertion + legacy detection, metadata kaos integration + atomic writes. 66 unit + 5 integration tests passing. cargo check clean."
}
```

**How to resume after restart:**
1. Read `checkpoints/latest.json`.
2. List `src/` to verify what already exists.
3. Continue from `next_module_group` and `in_progress_modules`.
4. Run `cargo check` to verify compilation baseline.

**Active task list (as of last update):**
- Phase 11 is in progress: translating remaining large peripheral modules (wire server, web, vis, rich UI shell, notifications, utils, background store, ACP, plugin manager, CLI commands).
- Build and test continuously with `cargo check` and `cargo test --lib`.
