# Phase 5: UI Layer Translation Plan

## Objective
Replace Python `prompt_toolkit` + `Rich` with a Rust terminal UI stack built on `ratatui` + `crossterm`. Provide interactive shell, non-interactive print mode, and ACP UI stubs.

## 5.1 `src/ui/theme.rs`

**Strategy:** Minimal theme container mirroring the Python theme registry.

```rust
/// Terminal theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "dark".into(),
        }
    }
}
```

## 5.2 `src/ui/print/mod.rs`

**Strategy:** Simple text emitter for piping and non-interactive output.

```rust
/// Print-mode UI renderer.
#[derive(Debug, Clone, Default)]
pub struct PrintUi;

impl PrintUi {
    pub fn render(&self, text: &str) {
        println!("{text}");
    }
}
```

## 5.3 `src/ui/acp/mod.rs`

**Strategy:** Placeholder struct for future ACP-specific UI rendering.

```rust
/// ACP (Agent Control Protocol) UI stub.
#[derive(Debug, Clone, Default)]
pub struct AcpUi;
```

## 5.4 `src/ui/shell/mod.rs`

**Strategy:** Full `ratatui` event-loop shell replacing `prompt_toolkit`.

### Key Design Decisions
- **Backend:** `CrosstermBackend` writing to `stderr` (or `stdout`).
- **Event loop:** `spawn_blocking` around `crossterm::event::read()` to bridge sync terminal events into the async runtime.
- **Layout:** Vertical split with a scrollable history pane on top and an input box below.
- **Cursor:** Manually positioned inside the input box after the `> ` prompt.

### Data Model
```rust
struct HistoryItem {
    role: &'static str,   // "user" | "assistant"
    content: String,
}

pub struct ShellUi {
    history: Vec<HistoryItem>,
    input: String,
    scroll_offset: u16,
}
```

### Event Handling
| Key | Action |
|-----|--------|
| `Enter` | Submit input, run soul turn, append assistant reply |
| `Ctrl+C` / `Ctrl+Q` / `Esc` | Exit shell |
| `Backspace` | Delete last character |
| `Up` / `Down` | Scroll history view |
| Any char | Append to input buffer |

### Integration with Soul
`ShellUi::run` accepts `&mut crate::app::KimiCLI`, builds a `ContentPart::Text` from the input line, calls `cli.run(parts).await`, and renders the `TurnOutcome::final_message` text into the history buffer.

### Lifecycle
1. `enable_raw_mode()`
2. Enter alternate screen + enable mouse capture
3. Run event loop
4. On exit: leave alternate screen, disable mouse, `disable_raw_mode()`, show cursor

## 5.5 `src/ui/shell/visualize/mod.rs`

**Strategy:** Reserved for future live-visualization widgets (Markdown blocks, diff panes, image previews). Currently a placeholder.

## Tracing Strategy for UI Layer
- `ShellUi::run` → `#[tracing::instrument(level = "info")]`
- Record shell start/stop and turn submission counts.
- `trace` level for individual key events.
