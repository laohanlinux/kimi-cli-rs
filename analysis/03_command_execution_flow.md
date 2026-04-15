# Command Execution Flow

## 1. CLI Entry to Shell Mode

```mermaid
flowchart TD
    Start([User types: kimi --model kimi-k2.5]) --> ParseArgs["cli/__init__.py: kimi() callback"]
    ParseArgs --> Validate{"Validate conflicts:<br/>--session vs --continue<br/>--print vs --shell"}
    Validate -->|invalid| Exit1["typer.Exit(1)"]
    Validate -->|valid| ResolveWD["resolve_work_dir()"]
    ResolveWD --> LoadSession{"session_id?"}
    LoadSession -->|provided| SessionFind["Session.find(work_dir, session_id)"]
    LoadSession -->|--continue| SessionContinue["Session.continue_(work_dir)"]
    LoadSession -->|new| SessionCreate["Session.create(work_dir)"]
    SessionFind --> BuildKimiCLI["KimiCLI.create(session, config, model, yolo, plan_mode)"]
    SessionContinue --> BuildKimiCLI
    SessionCreate --> BuildKimiCLI
    BuildKimiCLI --> ModeSelect{"UIMode?"}
    ModeSelect -->|shell| RunShell["instance.run_shell()"]
    ModeSelect -->|print| RunPrint["instance.run_print()"]
    ModeSelect -->|acp| RunACP["instance.run_acp()"]
    ModeSelect -->|wire| RunWire["instance.run_wire_stdio()"]
    RunShell --> ShellRun["Shell.run()<br/>interactive REPL loop"]
    RunPrint --> PrintRun["Print.run()<br/>stdin → soul → stdout"]
    RunACP --> ACPRun["ACP.run()<br/>deprecated stub"]
    RunWire --> WireRun["WireServer.serve()<br/>JSON-RPC over stdio"]
```

## 2. `KimiCLI.create()` Boot Sequence

```mermaid
flowchart TD
    A[KimiCLI.create] --> B["1. load_config()<br/>config.py:247"]
    B --> C["2. Create OAuthManager"]
    C --> D["3. Resolve model + provider<br/>from config or env"]
    D --> E["4. create_llm()<br/>llm.py"]
    E --> F["5. Runtime.create()<br/>soul/agent.py:185"]
    F --> F1["5a. Build BuiltinSystemPromptArgs"]
    F --> F2["5b. Load skills"]
    F --> F3["5c. Init BackgroundTaskManager"]
    F --> F4["5d. Init NotificationManager"]
    F --> F5["5e. Init SubagentStore"]
    F --> F6["5f. Init ApprovalRuntime + RootWireHub"]
    F --> F7["5g. Reconcile background tasks"]
    F --> F8["5h. Cleanup stale foreground subagents"]
    F8 --> G["6. load_agent()<br/>soul/agent.py:393"]
    G --> G1["6a. Parse YAML agent spec"]
    G --> G2["6b. Create KimiToolset"]
    G --> G3["6c. Load builtin tools"]
    G --> G4["6d. Load plugin tools"]
    G --> G5["6e. Load MCP tools"]
    G5 --> H["7. Context.restore()<br/>soul/context.py:30"]
    H --> I["8. Create KimiSoul"]
    I --> J["9. HookEngine injection"]
    J --> K["Return KimiCLI"]
```

## 3. Shell REPL Event Loop

```mermaid
sequenceDiagram
    participant Shell as Shell
    participant Prompt as CustomPromptSession
    participant Router as _route_prompt_events
    participant Idle as idle_events queue
    participant Watcher as bg_watcher
    participant Soul as run_soul_command

    Shell->>Prompt: create_prompt_session()
    Shell->>Router: asyncio.create_task(_route_prompt_events)
    Shell->>Watcher: start background watcher
    loop REPL
        Shell->>Idle: wait for next event
        alt Background task completed
            Watcher-->>Idle: task completion
            Shell->>Soul: run system reminder
        else User input
            Router->>Idle: _PromptEvent
            Shell->>Shell: classify_input()
            alt Shell command
                Shell->>Shell: execute_shell_command()
            else Slash command
                Shell->>Shell: execute_slash_command()
            else Agent message
                Shell->>Soul: run_soul_command(text)
            end
        end
    end
```

## 4. Shell Input Classification

```mermaid
flowchart TD
    Input[User input string] --> Empty{"is empty?"}
    Empty -->|yes| Ignore["Ignore"]
    Empty -->|no| Prefix{"prefix?"}
    Prefix -->|!| ShellCmd["Shell command<br/>(strip !)"]
    Prefix -->|/| SlashCmd["Slash command<br/>e.g. /yolo, /plan"]
    Prefix -->|!mcp| MCPStatus["Show MCP status"]
    Prefix -->|default| AgentMsg["Agent message"]
```

## 5. Web Mode Execution Flow

```mermaid
flowchart TD
    A[kimi web] --> B["run_web_server()<br/>web/app.py:228"]
    B --> C["Generate session token"]
    C --> D["Print banner with URLs"]
    D --> E["uvicorn.run(create_app)"]
    E --> F["FastAPI lifespan starts<br/>KimiCLIRunner"]
    F --> G["User opens browser"]
    G --> H["GET / (SPA fallback)"]
    H --> I["WebSocket /api/sessions/{id}/stream"]
    I --> J["session_stream()<br/>web/api/sessions.py:872"]
    J --> K{"History exists?"}
    K -->|yes| Replay["replay_history()<br/>from wire.jsonl"]
    K -->|no| Start["Start worker subprocess"]
    Replay --> L["Forward WS ↔ worker stdin/stdout"]
    Start --> L
    L --> M["Worker runs wire_stdio<br/>→ KimiCLI → KimiSoul"]
```

## 6. ACP Server Execution Flow

```mermaid
sequenceDiagram
    participant Client as MCP Client (Cursor/Claude)
    participant ACP as ACPServer
    participant Auth as _check_auth()
    participant Session as ACP Session
    participant Kimi as KimiCLI
    participant Soul as KimiSoul

    Client->>ACP: initialize()
    ACP->>ACP: negotiate_version()
    ACP-->>Client: capabilities
    Client->>ACP: new_session(cwd, mcp_servers)
    ACP->>Auth: load_tokens()
    alt No valid token
        Auth-->>ACP: auth_required error
        ACP-->>Client: Error + terminal auth URL
    else Valid token
        ACP->>Kimi: KimiCLI.create(session, mcp_configs)
        ACP->>Session: ACPSession(kimi_cli)
        ACP-->>Client: Session info + models
    end
    Client->>Session: prompt(content_blocks)
    Session->>Session: acp_blocks_to_content_parts()
    Session->>Soul: kimi_cli.run()
    Soul-->>Session: WireMessage stream
    Session->>Session: Map to ACP updates
    Session-->>Client: agent_message_chunk, tool_call, etc.
```
