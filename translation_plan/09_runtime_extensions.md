# Phase 9: Runtime Extensions Translation Plan

## Objective
Document and complete the translation of runtime extension modules that were omitted from Phase 4: notifications, approval runtime, hooks, auth, and remaining soul subsystems (`btw`, `denwa_renji`).

## 9.1 Notifications (`src/notifications/`)

**Current State:** `NotificationManager` is a stub with no-op `new()` and `default()`.

**Rust:**
```rust
/// Manages user notifications.
#[derive(Debug, Clone)]
pub struct NotificationManager;

impl NotificationManager {
    pub fn new(_root: &Path, _config: crate::config::NotificationConfig) -> Self {
        Self
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self
    }
}
```

**Config Tie-in:** `NotificationConfig` is present in `Config` with a `Default` implementation (empty).

**Next Steps:**
- Implement a notification queue (e.g., `tokio::sync::mpsc`) for delivering system-to-user alerts.
- Integrate with the soul loop so notifications are injected into the context or displayed via the wire.

## 9.2 Approval Runtime (`src/approval_runtime/`)

**Current State:** `ApprovalRuntime` is a no-op struct.

**Rust:**
```rust
/// Runtime for evaluating approval rules.
#[derive(Debug, Clone, Default)]
pub struct ApprovalRuntime;

impl ApprovalRuntime {
    pub fn bind_root_wire_hub(&self, _root_wire_hub: &crate::wire::root_hub::RootWireHub) {}
}
```

**Next Steps:**
- Add rule evaluation engine (e.g., regex or policy-based matching on tool names and arguments).
- Wire into `KimiSoul::step()` so that when `approval_runtime` is present, it evaluates rules *before* publishing the `ApprovalRequest` to the wire.
- Support auto-approve patterns and deny-lists.

## 9.3 Hooks Engine (`src/hooks/`)

**Current State:** `HookEngine` exists but always returns `HookAction::Allow`.

**Rust:**
```rust
pub enum HookAction {
    Allow,
    Block { reason: String },
}

#[derive(Debug, Clone, Default)]
pub struct HookEngine;

impl HookEngine {
    pub async fn trigger(
        &self,
        _hook_name: &str,
        _tool_name: &str,
        _arguments: serde_json::Value,
    ) -> crate::error::Result<HookAction> {
        Ok(HookAction::Allow)
    }
}
```

**Integration Points:**
- `KimiToolset::handle()` already calls `PreToolUse` and `PostToolUse` hooks.
- `KimiSoul::new()` initializes a default `HookEngine`.

**Next Steps:**
- Parse `HookDef` entries from `Config` (each hook has a name, event filter, and action).
- Implement blocking logic based on tool-name wildcards and argument predicates.
- Add async script/URL invocation for external hooks.

## 9.4 Auth / OAuth (`src/auth/`)

**Current State:** `OAuthManager` is a no-op struct.

**Rust:**
```rust
/// OAuth credential and token manager.
#[derive(Debug, Clone, Default)]
pub struct OAuthManager;
```

**Config Tie-in:** `OAuthRef` is defined in `config.rs` and attached to `LlmProvider` and other services.

**Next Steps:**
- Integrate `oauth2` crate flows (authorization code, device code, or client credentials).
- Store tokens securely (keyring via `keyring` crate or encrypted file).
- Expose `get_token(storage, key)` API that resolves the `OAuthRef` at runtime.

## 9.5 Soul / BTW (`src/soul/btw.rs`)

**Current State:** **MISSING** — not present in `src/soul/` or `src/soul/mod.rs`.

**Python Origin:** `soul/btw.py` — "By The Way" notification injection.

**Proposed Rust:**
```rust
/// BTW (By The Way) notification injector.
#[derive(Debug, Clone, Default)]
pub struct BtwNotifier;

impl BtwNotifier {
    /// Checks if a BTW notification should be injected into the current turn.
    pub fn should_notify(&self, _context: &crate::soul::context::Context) -> Option<String> {
        None
    }
}
```

**Next Steps:**
- Create `src/soul/btw.rs`.
- Add `pub mod btw;` to `src/soul/mod.rs`.
- Wire `BtwBegin` / `BtwEnd` wire messages into the soul loop when a BTW is triggered.

## 9.6 Soul / Denwa Renji (`src/soul/denwa_renji.rs`)

**Current State:** Translated and functional.

**Rust:**
```rust
/// D-Mail message for time-travel context reversion.
#[derive(Debug, Clone)]
pub struct DMail {
    pub message: String,
    pub checkpoint_id: usize,
}

/// Time-travel phone booth for reverting context to checkpoints.
#[derive(Debug, Clone, Default)]
pub struct DenwaRenji {
    pending_dmail: Option<DMail>,
    n_checkpoints: usize,
}

impl DenwaRenji {
    pub fn send_dmail(&mut self, dmail: DMail) -> crate::error::Result<()> {
        if self.pending_dmail.is_some() {
            return Err(crate::error::KimiCliError::Generic(
                "Only one D-Mail can be sent at a time".into(),
            ));
        }
        if dmail.checkpoint_id >= self.n_checkpoints {
            return Err(crate::error::KimiCliError::Generic(
                "There is no checkpoint with the given ID".into(),
            ));
        }
        self.pending_dmail = Some(dmail);
        Ok(())
    }

    pub fn set_n_checkpoints(&mut self, n: usize) {
        self.n_checkpoints = n;
    }

    pub fn fetch_pending_dmail(&mut self) -> Option<DMail> {
        self.pending_dmail.take()
    }
}
```

**Integration Points:**
- `Context::checkpoint()` increments the checkpoint counter.
- `tools/dmail` (stub) should construct a `DMail` and call `DenwaRenji::send_dmail()`.
- `KimiSoul::agent_loop()` should check for a pending D-Mail after each step and revert context if present.

## 9.7 Config Support

Ensure the following config structs have `Default` implementations and are wired into `Runtime::create()`:

- `BackgroundConfig` — max concurrent tasks, shell path
- `NotificationConfig` — enabled channels, sound, desktop
- `McpConfig` — server list, timeouts
- `HookDef` — name, event, pattern, action

## Tracing Strategy for Runtime Extensions
- `HookEngine::trigger` → `debug` level with hook name, tool name, and action outcome.
- `ApprovalRuntime` rule evaluation → `debug` level.
- `OAuthManager` token fetch → `info` level (redact secrets).
- `BtwNotifier::should_notify` → `trace` level.
- `DenwaRenji::send_dmail` → `info` level with checkpoint ID.
