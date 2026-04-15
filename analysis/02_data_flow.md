# Data Flow Diagrams

## 1. User Message to LLM Response Flow

```mermaid
sequenceDiagram
    actor User
    participant Shell as Shell UI
    participant KimiCLI as KimiCLI (app.py)
    participant run_soul as run_soul()
    participant KimiSoul as KimiSoul
    participant Context as Context (JSONL)
    participant Toolset as KimiToolset
    participant LLM as LLM (kosong)
    participant Wire as Wire Channel

    User->>Shell: Type message + Enter
    Shell->>KimiCLI: shell.run(command)
    KimiCLI->>run_soul: run_soul(soul, input, ui_loop_fn, cancel_event)
    run_soul->>KimiSoul: soul.run(user_input)
    KimiSoul->>KimiSoul: _turn(user_message)
    KimiSoul->>Context: append_message(user)
    KimiSoul->>KimiSoul: _agent_loop()
    loop Up to max_steps_per_turn
        KimiSoul->>KimiSoul: _step()
        KimiSoul->>Context: normalize_history()
        KimiSoul->>LLM: kosong.step(provider, system_prompt, toolset, history)
        LLM-->>KimiSoul: StepResult (message + tool_calls)
        KimiSoul->>Context: update_token_count(usage)
        alt Has tool calls
            KimiSoul->>Toolset: handle(tool_call)
            Toolset-->>KimiSoul: ToolResult
            KimiSoul->>Context: append_message(assistant + tool results)
        else No tool calls
            KimiSoul-->>KimiSoul: return StepOutcome
        end
    end
    KimiSoul-->>run_soul: TurnOutcome
    run_soul-->>KimiCLI: Complete
    KimiCLI-->>Shell: Done
    Shell-->>User: Show response
```

## 2. Wire Message Data Flow

```mermaid
sequenceDiagram
    participant Soul as KimiSoul
    participant WireSS as WireSoulSide
    participant RawQ as Raw BroadcastQueue
    participant MergeQ as Merged BroadcastQueue
    participant WireUI as WireUISide
    participant LiveView as _LiveView / _PromptLiveView
    participant RootHub as RootWireHub

    Soul->>WireSS: send(TurnBegin)
    WireSS->>RawQ: publish(TurnBegin)
    WireSS->>MergeQ: flush_merge_buffer() then publish(TurnBegin)
    RawQ-->>WireUI: TurnBegin
    MergeQ-->>WireUI: TurnBegin
    WireUI-->>LiveView: dispatch_wire_message()

    Soul->>WireSS: send(TextPart("Hello"))
    WireSS->>WireSS: merge_buffer.append(TextPart)
    Soul->>WireSS: send(TextPart(" World"))
    WireSS->>WireSS: merge_buffer.extend(TextPart)
    Soul->>WireSS: send(ToolCall)
    WireSS->>MergeQ: flush merged TextPart("Hello World")
    WireSS->>MergeQ: publish(ToolCall)
    MergeQ-->>WireUI: TextPart + ToolCall

    RootHub->>RawQ: publish(ApprovalRequest)
    RawQ-->>WireUI: ApprovalRequest
```

## 3. Session Persistence Data Flow

```mermaid
graph LR
    subgraph Memory["In-Memory"]
        K[KimiSoul]
        C[Context<br/>_history list]
        S[SessionState<br/>Pydantic model]
    end

    subgraph Disk["On Disk"]
        CF["context.jsonl<br/>per session"]
        WF["wire.jsonl<br/>per session"]
        SF["state.json<br/>per session"]
        MF["kimi.json<br/>global metadata"]
        CONF["config.toml<br/>global config"]
    end

    K -->|append_message| C
    C -->|write JSONL line| CF
    K -->|wire_send| WF
    K -->|save_state| S
    S -->|atomic JSON write| SF
    Session -->|load/save| MF
    Config -->|TOML read/write| CONF
```

## 4. Configuration Data Flow

```mermaid
graph TD
    A["User runs: kimi --config /path/to/config.toml"] --> B["cli/__init__.py: parse args"]
    B --> C{"config provided?"}
    C -->|yes| D["load_config(path)"]
    C -->|no| E["get_config_file() → ~/.kimi/config.toml"]
    E -->|exists| D
    E -->|missing| F["get_default_config()"]
    F -->|save| G["create ~/.kimi/config.toml"]
    D -->|validate| H["Config.model_validate(data)"]
    H -->|invalid| I["raise ConfigError"]
    H -->|valid| J["Config object"]
    J -->|inject| K["KimiCLI.create(config=...)"]
```

## 5. OAuth Token Data Flow

```mermaid
sequenceDiagram
    participant User
    participant CLI as cli/login
    participant OAuth as OAuthManager
    participant AuthSvr as auth.kimi.com
    participant File as ~/.kimi/oauth.json
    participant LLM as LLM Provider

    User->>CLI: kimi login
    CLI->>AuthSvr: POST /api/oauth/device_authorization
    AuthSvr-->>CLI: device_code, user_code, verification_uri
    CLI->>User: Open browser, enter code
    loop Poll until complete
        CLI->>AuthSvr: POST /api/oauth/token
        AuthSvr-->>CLI: pending / access_token + refresh_token
    end
    CLI->>File: Save tokens
    CLI->>OAuth: OAuthManager(config)
    Note over OAuth: resolve_api_key() reads file
    OAuth->>LLM: Inject Bearer token on requests
    OAuth->>OAuth: Auto-refresh every 60s
```

## 6. Background Task Data Flow

```mermaid
graph TB
    subgraph Create["Task Creation"]
        U[User sends shell command<br/>with run_in_background=true]
        T[Shell tool]
        T-->|runtime.background_tasks.create_bash_task()| M[BackgroundTaskManager]
        M-->|persist| TS[BackgroundTaskStore]
        M-->|spawn subprocess| W[Worker Process]
    end

    subgraph Monitor["Monitoring"]
        W-->|stdout/stderr| F[Task log files]
        TS-->|heartbeat updates| F2[Task state files]
    end

    subgraph Query["Query / Stop"]
        U2[User calls TaskList/TaskOutput/TaskStop]
        T2[Background tools]
        T2-->|read store + files| TS
        TS-->|return TaskView| T2
        T2-->|display| U2
    end

    subgraph Reconcile["Reconciliation"]
        KimiCLI-->|on startup| M
        M-->|scan disk + kill stale| W
    end
```

## 7. Subagent Data Flow

```mermaid
sequenceDiagram
    participant Soul as KimiSoul
    participant Toolset as KimiToolset
    participant LM as LaborMarket
    participant Builder as SubagentBuilder
    participant Runner as ForegroundSubagentRunner
    participant SubSoul as Subagent KimiSoul
    participant Store as SubagentStore

    Soul->>Toolset: _bind_subagent_tools()
    Toolset->>LM: list_types()
    LM-->>Toolset: AgentTypeDefinition[]
    Toolset->>Toolset: Register AgentLaunchSpec tools

    LLM->>Toolset: Call agent tool
    Toolset->>Builder: build_builtin_instance(spec)
    Builder->>Runner: prepare + run
    Runner->>SubSoul: prepare_soul() → new KimiSoul
    Runner->>SubSoul: run_soul_checked()
    SubSoul-->>Runner: TurnOutcome
    Runner->>Store: update_instance(status, output)
    Runner-->>Toolset: ToolResult(summary)
    Toolset-->>Soul: ToolResult
```
