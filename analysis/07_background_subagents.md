# Background Tasks and Subagents Architecture

## 1. Background Task Manager

```mermaid
graph TB
    subgraph BTM["BackgroundTaskManager (background/manager.py:34)"]
        Create[create_bash_task]
        CreateA[create_agent_task]
        List[list_tasks]
        Stop[kill_task / kill_all_active]
        Recon[reconcile]
    end

    subgraph Store["BackgroundTaskStore (background/store.py:30)"]
        Add[add]
        Update[update_runtime / update_control]
        Get[get_task]
        ListS[list_tasks]
    end

    subgraph Worker["Worker Process (background/worker.py)"]
        Main[__main__: run_background_task]
        Bash[execute bash command]
        Agent[BackgroundAgentRunner.run]
        Heartbeat[heartbeat to store]
    end

    BTM --> Store
    BTM --spawn subprocess--> Worker
    Worker --write stdout/stderr--> LogFiles[Task log files]
    Worker --heartbeat--> Store
```

## 2. Background Task Lifecycle

```mermaid
stateDiagram-v2
    [*] --> pending: create_bash_task / create_agent_task
    pending --> running: subprocess starts
    running --> completed: exit code 0
    running --> failed: non-zero exit / exception
    running --> killed: kill_task called
    running --> stale: no heartbeat > 15s
    stale --> killed: reconcile()
    completed --> [*]
    failed --> [*]
    killed --> [*]
```

## 3. Subagent Architecture

```mermaid
graph TB
    subgraph Registry["LaborMarket (subagents/registry.py:8)"]
        Register[register_builtin_type]
        List[list_types]
    end

    subgraph Builder["SubagentBuilder (subagents/builder.py:8)"]
        Build[build_builtin_instance]
        Resolve[resolve_effective_model]
    end

    subgraph Runner["ForegroundSubagentRunner (subagents/runner.py:197)"]
        Prepare[_prepare_instance]
        Run[_run_core]
        UILoop[_make_ui_loop_fn]
    end

    subgraph Store2["SubagentStore (subagents/store.py:64)"]
        Add2[add_instance]
        Update2[update_instance]
        Get2[get_instance]
    end

    subgraph Core["SubagentCore (subagents/core.py)"]
        PrepS[prepare_soul]
    end

    Registry --AgentTypeDefinition--> Builder
    Builder --PreparedInstance--> Runner
    Runner --> Core
    Core --KimiSoul--> Runner
    Runner --> Store2
```

## 4. Foreground Subagent Execution Flow

```mermaid
sequenceDiagram
    participant Soul as KimiSoul
    participant Toolset as KimiToolset
    participant Runner as ForegroundSubagentRunner
    participant Core as prepare_soul
    participant SubSoul as Subagent KimiSoul
    participant Output as SubagentOutputWriter
    participant Store as SubagentStore

    Soul->>Toolset: agent tool call
    Toolset->>Runner: run
    Runner->>Core: prepare_soul(spec)
    Core->>Core: Create isolated Session
    Core->>Core: Build Runtime with scoped tools
    Core->>Core: load_agent with subagent spec
    Core-->>Runner: PreparedInstance
    Runner->>Store: add_instance(running_foreground)
    Runner->>SubSoul: run_soul_checked(prompt)
    SubSoul-->>Runner: TurnOutcome
    Runner->>Output: write output + summary
    Runner->>Store: update_instance(completed/failed)
    Runner-->>Toolset: ToolResult(summary)
```

## 5. Background Agent Runner Flow

```mermaid
flowchart TD
    A[BackgroundAgentRunner.run] --> B[Attach ApprovalRuntime listener]
    B --> C[_run_core]
    C --> D[Prepare subagent soul]
    D --> E[run_soul_checked with output writer]
    E --> F{Approval event from root?}
    F -->|yes| G[_apply_approval_runtime_event]
    G --> H[Forward approval request to subagent runtime]
    H --> I[Subagent resolves approval]
    I --> E
    F -->|no| J[Loop continues]
    J -->|turn complete| K[Write final summary]
```

## 6. Notification System Flow

```mermaid
sequenceDiagram
    participant Producer as Any Component
    participant NM as NotificationManager
    participant NS as NotificationStore
    participant Sink as NotificationSink (UI)

    Producer->>NM: publish(NotificationEvent)
    NM->>NS: Persist event + delivery state
    NM->>NM: dedupe by dedupe_key
    NM-->>Producer: NotificationView

    Sink->>NM: claim_for_sink(sink_id)
    NM->>NS: Query pending for sink
    NS-->>NM: NotificationView[]
    NM-->>Sink: Pending notifications

    Sink->>NM: deliver_pending(async)
    NM->>Sink: Send Notification wire messages
    Sink->>NM: ack(notification_id)
    NM->>NS: Mark delivered
```

## 7. Approval Runtime Architecture

```mermaid
graph TB
    subgraph AR["ApprovalRuntime (approval_runtime/runtime.py:48)"]
        Create[create_request]
        Wait[wait_for_response]
        Resolve[resolve]
        List[list_pending]
        Cancel[cancel_all_for_source]
    end

    subgraph ARModel["ApprovalRuntimeEvent (approval_runtime/models.py:40)"]
        Req[ApprovalRequestRecord]
        Src[ApprovalSource]
        Ev[ApprovalRuntimeEvent]
    end

    Tool[Tool Execution] --request()--> Create
    Create --store--> Req
    Create --publish--> RootHub[RootWireHub]
    UI[User UI] --approve/reject--> Resolve
    Resolve --> Wait
    Wait --> Tool
```
