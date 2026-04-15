# UI Layer Architecture

## 1. UI Mode Hierarchy

```mermaid
graph TB
    subgraph Modes["Run Modes"]
        S[Shell Mode]
        P[Print Mode]
        A[ACP Mode]
        W[Wire Mode]
    end

    subgraph ShellUI["Shell UI (ui/shell/)"]
        Sh1["Shell (ui/shell/__init__.py:178)"]
        Sh2["CustomPromptSession (prompt.py:1166)"]
        Sh3["_LiveView (_live_view.py:102)"]
        Sh4["_PromptLiveView (_interactive.py:54)"]
        Sh5["ApprovalPanel (_approval_panel.py:48)"]
        Sh6["QuestionPanel (_question_panel.py:24)"]
        Sh7["BtwPanel (_btw_panel.py:63)"]
    end

    subgraph PrintUI["Print UI (ui/print/)"]
        P1["Print (ui/print/__init__.py:34)"]
        P2["TextPrinter / JsonPrinter (visualize.py)"]
    end

    subgraph ACPUI["ACP UI (ui/acp/)"]
        A1["ACP (ui/acp/__init__.py:90)"]
        A2["ACPServerSingleSession (stub)"]
    end

    subgraph WireUI["Wire Server (wire/server.py)"]
        W1["WireServer"]
        W2["JSON-RPC read/write loops"]
    end

    S --> Sh1
    P --> P1
    A --> A1
    W --> W1
    Sh1 --> Sh2
    Sh1 --> Sh3
    Sh3 --> Sh4
    Sh4 --> Sh5
    Sh4 --> Sh6
    Sh4 --> Sh7
    P1 --> P2
    W1 --> W2
```

## 2. Interactive Shell Event Loop

```mermaid
flowchart TD
    Start([Shell.run()]) --> Init["1. Create CustomPromptSession
2. Start _route_prompt_events task
3. Start bg_watcher"]
    Init --> Loop["while True"]
    Loop --> Wait["bg_watcher.wait_for_next(idle_events)"]
    Wait --> Result{"Event type?"}
    Result -->|input| Classify["classify_input()"]
    Classify -->|agent msg| RunSoul["run_soul_command(text)"]
    Classify -->|slash cmd| Slash["execute_slash_command()"]
    Classify -->|shell cmd| ShellCmd["execute_shell_command()"]
    Result -->|interrupt| Interrupt["Handle Ctrl+C
cancel_event.set()"]
    Result -->|background done| BgDone["run_soul_command(
<system-reminder>...)"]
    RunSoul --> Viz["visualize(wire.ui_side)"]
    Viz --> Live["_PromptLiveView"]
    Live --> Render["dispatch_wire_message()"]
    Render --> Blocks["Update _ContentBlock / _ToolCallBlock"]
    RunSoul -->|complete| Loop
```

## 3. Prompt Toolkit UI State Machine

```mermaid
stateDiagram-v2
    [*] --> NORMAL_INPUT: startup
    NORMAL_INPUT --> MODAL_HIDDEN_INPUT: approval modal shown
    MODAL_HIDDEN_INPUT --> NORMAL_INPUT: modal dismissed
    NORMAL_INPUT --> MODAL_TEXT_INPUT: question modal shown
    MODAL_TEXT_INPUT --> NORMAL_INPUT: question answered
    NORMAL_INPUT --> RUNNING_PROMPT: agent turn starts
    RUNNING_PROMPT --> NORMAL_INPUT: turn ends
```

## 4. Live View Rendering Pipeline

```mermaid
flowchart LR
    WireUI --WireMessage--> LV["_LiveView (Rich Live)"]
    LV --> DM[dispatch_wire_message]
    DM --> CB["_ContentBlock
(incremental markdown)"]
    DM --> TB["_ToolCallBlock"]
    DM --> SB["_StatusBlock"]
    DM --> NB["_NotificationBlock"]
    CB --flush confirmed--> Console["KimiConsole history"]
    TB --> Console
    LV --render--> LivePanel["Rich Live Panel"]
```

## 5. Print Mode Flow

```mermaid
flowchart TD
    A[Print.run] --> B{"command provided?"}
    B -->|no| C["Read stdin if piped"]
    B -->|yes| D[Use command]
    C --> E[run_soul(input, visualize)]
    D --> E
    E --> F[visualize(wire, output_format)]
    F --> G{"output_format?"}
    G -->|text| H[TextPrinter]
    G -->|stream-json| I[JsonPrinter]
    H --> J[rich.print to stdout]
    I --> K[json.dumps to stdout]
```

## 6. ACP to Internal Wire Mapping

```mermaid
graph LR
    ACPText["TextContentBlock"] --> TextPart
    ACPImage["ImageContentBlock"] --> ImageURLPart
    ACPResource["EmbeddedResource"] --> TextPart

    TextPart --> ACPMsg["agent_message_chunk"]
    ThinkPart --> ACPThought["agent_thought_chunk"]
    ToolCall --> ACPTool["tool_call / tool_call_update"]
    ToolResult --> ACPTool2["tool_call_update completed/failed"]
    ApprovalRequest --> ACPPerm["request_permission"]
    Notification --> ACPNotif["[Notification] prefix text"]
```
