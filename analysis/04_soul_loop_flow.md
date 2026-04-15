# Soul / Agent Loop Flow

## 1. KimiSoul Turn Lifecycle

```mermaid
flowchart TD
    Start([run_soul() called]) --> TurnBegin["KimiSoul.run()
soul/kimisoul.py:464"]
    TurnBegin --> Hooks["UserPromptSubmit hook
block?"]
    Hooks -->|blocked| BlockedTurn["Send TurnBegin
Blocked reason
TurnEnd
Return"]
    Hooks -->|allowed| CheckSlash{"Slash command?"}
    CheckSlash -->|yes| SlashDispatch["Dispatch to
SlashCommand handler"]
    CheckSlash -->|no| CheckRalph{"Ralph mode?
max_ralph_iterations != 0"}
    CheckRalph -->|yes| RalphLoop["FlowRunner.loop()"]
    CheckRalph -->|no| NormalTurn["_turn(user_message)"]
    NormalTurn --> AgentLoop["_agent_loop()"]
    AgentLoop --> StepLoop["For step in 1..max_steps_per_turn"]
    StepLoop --> StepBegin["Send StepBegin wire msg"]
    StepBegin --> CompactCheck{"Context > trigger?"}
    CompactCheck -->|yes| Compact["compact_context()
CompactionBegin/End"]
    CompactCheck -->|no| StepExec["_step()"]
    Compact --> StepExec
    StepExec --> Outcome{"Outcome?"}
    Outcome -->|continue| StepLoop
    Outcome -->|stop| TurnEnd["Send TurnEnd wire msg"]
    TurnEnd --> ReturnTurn["Return TurnOutcome"]
```

## 2. Single Step Internal Flow

```mermaid
flowchart TD
    A[_step()] --> B["Deliver pending notifications
(if root role)"]
    B --> C["Collect dynamic injections
PlanMode / YoloMode"]
    C --> D["normalize_history()"]
    D --> E["kosong.step(
  provider,
  system_prompt,
  toolset,
  history
)"]
    E --> F["Retry on retryable errors
(tenacity)"]
    F --> G["Log token usage
Update context token count"]
    G --> H["Wait for tool results
result.tool_results()"]
    H --> I["_grow_context()"]
    I --> J{"Rejection without feedback?"}
    J -->|yes| K["Stop turn
return StepOutcome(stop)"]
    J -->|no| L{"DenwaRenji D-Mail?"}
    L -->|yes| M["Raise BackToTheFuture
Revert context"]
    L -->|no| N{"Has tool calls?"}
    N -->|yes| O["return None (continue loop)"]
    N -->|no| P["return StepOutcome(no_tool_calls)"]
```

## 3. Context Compaction Flow

```mermaid
sequenceDiagram
    participant Soul as KimiSoul
    participant Context as Context
    participant Compaction as SimpleCompaction
    participant LLM as LLM Provider

    Soul->>Soul: Check context_tokens >= max * ratio
    Soul->>Compaction: compact_context()
    Compaction->>Context: prepare(preserve_last_n=2)
    Compaction->>Compaction: Identify cutoff point
    Compaction->>LLM: kosong.step with compaction system prompt
    LLM-->>Compaction: Summary text
    Compaction->>Context: checkpoint()
    Compaction->>Context: append_message(compacted summary)
    Compaction->>Context: Replay preserved messages
    Compaction-->>Soul: Done
```

## 4. Plan Mode State Machine

```mermaid
stateDiagram-v2
    [*] --> Inactive: session starts
    Inactive --> Activating: EnterPlanMode tool called
    Activating --> Active: User approves
    Active --> Inactive: ExitPlanMode tool called + approved
    Active --> Active: Every LLM step gets plan injection
    Inactive --> Inactive: Normal operation

    note right of Active
        Dynamic injection provider
        injects plan reminders before
        each LLM step
    end note
```

## 5. DenwaRenji (Time Travel) Flow

```mermaid
sequenceDiagram
    participant Tool as SendDMail Tool
    participant Denwa as DenwaRenji
    participant Soul as KimiSoul
    participant Context as Context

    Tool->>Denwa: send_dmail(target_checkpoint_id, message)
    Denwa->>Denwa: Validate checkpoint exists
    Denwa-->>Tool: Success
    Soul->>Soul: Catch D-Mail in _step()
    Soul->>Context: revert_to(target_checkpoint_id)
    Context->>Context: Rotate current file to backup
    Context->>Context: Replay up to checkpoint
    Context-->>Soul: Restored history
    Soul->>Soul: Append D-Mail as user message
    Soul->>Soul: Restart _agent_loop()
```
