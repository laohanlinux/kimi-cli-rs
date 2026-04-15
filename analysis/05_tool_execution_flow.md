# Tool Execution and Approval Flow

## 1. Tool Invocation Sequence

```mermaid
sequenceDiagram
    participant LLM as LLM (kosong)
    participant Step as kosong.step()
    participant Toolset as KimiToolset
    participant Hook as HookEngine
    participant Tool as Tool Instance
    participant Runtime as Runtime

    LLM->>Step: Generates ToolCall
    Step->>Toolset: handle(tool_call)
    Toolset->>Toolset: Set current_tool_call ContextVar
    Toolset->>Toolset: Find tool by name
    alt Tool not found
        Toolset-->>Step: ToolNotFoundError
    else Parse args failed
        Toolset-->>Step: ToolParseError
    else Tool found
        Toolset->>Hook: trigger(PreToolUse, ...)
        Hook-->>Toolset: block / allow
        alt Blocked
            Toolset-->>Step: ToolError(blocked)
        else Allowed
            Toolset->>Tool: tool.call(arguments)
            alt Execution error
                Tool-->>Toolset: Exception
                Toolset->>Hook: trigger(PostToolUseFailure, ...)
                Toolset-->>Step: ToolError
            else Success
                Tool-->>Toolset: ToolReturnValue
                Toolset->>Hook: fire PostToolUse (async)
                Toolset-->>Step: ToolResult
            end
        end
    end
```

## 2. Approval Flow (Foreground Tool)

```mermaid
sequenceDiagram
    participant Tool as Tool (e.g. Shell)
    participant Approval as Approval Facade
    participant AR as ApprovalRuntime
    participant RootHub as RootWireHub
    participant UI as Shell UI / WebSocket
    participant User as Human User

    Tool->>Approval: request(action, description, display)
    alt YOLO mode
        Approval-->>Tool: auto-approved
    else Auto-approve list matches
        Approval-->>Tool: auto-approved
    else Needs approval
        Approval->>AR: create_request(...)
        AR->>RootHub: publish(ApprovalRequest)
        RootHub-->>UI: Show approval modal
        UI->>User: Display action + diff/preview
        User->>UI: Approve / Reject / Approve for session
        UI->>AR: resolve(request_id, response)
        AR-->>Approval: (response, feedback)
        alt Approve for session
            Approval->>Approval: Add to auto_approve_actions
        end
        Approval-->>Tool: ApprovalResult
    end
```

## 3. MCP Tool Execution Flow

```mermaid
flowchart TD
    A[LLM calls MCP tool] --> B[KimiToolset.handle]
    B --> C[Find MCPTool]
    C --> D{YOLO or auto-approve?}
    D -->|no| E[Approval.request]
    D -->|yes| F[fastmcp.Client.call_tool]
    E -->|approved| F
    F --> G[convert_mcp_tool_result]
    G --> H{Output > 100K?}
    H -->|yes| I[Truncate / return ToolError]
    H -->|no| J[Return ToolResult]
```

## 4. External Tool (Wire) Execution Flow

```mermaid
sequenceDiagram
    participant Soul as KimiSoul
    participant Toolset as KimiToolset
    participant Ext as WireExternalTool
    participant WireSS as WireSoulSide
    participant WireSvr as WireServer
    participant Client as JSON-RPC Client

    Soul->>Toolset: handle(external_tool_call)
    Toolset->>Ext: __call__(arguments)
    Ext->>WireSS: send(ToolCallRequest)
    WireSS->>WireSvr: Forward via Wire protocol
    WireSvr->>Client: JSONRPCRequestMessage(tool_call)
    Client-->>WireSvr: JSONRPCSuccessResponse(result)
    WireSvr->>Ext: request.resolve(return_value)
    Ext-->>Toolset: ToolReturnValue
    Toolset-->>Soul: ToolResult
```

## 5. Background Shell Task Creation Flow

```mermaid
flowchart TD
    A[Shell tool receives run_in_background=true] --> B[Approval.request]
    B -->|approved| C[BackgroundTaskManager.create_bash_task]
    C --> D[BackgroundTaskStore.add]
    D --> E[Spawn worker subprocess
    python -m kimi_cli __background-task-worker]
    E --> F[Worker reads TaskSpec
    executes bash command]
    F --> G[Write stdout/stderr to log files]
    G --> H[Heartbeat updates to store]
```

## 6. File Edit Tool Approval Exception (Plan Mode)

```mermaid
flowchart TD
    A[WriteFile / StrReplaceFile called] --> B{Is target the current plan file?}
    B -->|yes| C[Auto-approve
plan_mode inspect_plan_edit_target]
    B -->|no| D[Normal Approval.request]
    C --> E[Execute file edit]
    D -->|approved| E
    D -->|rejected| F[Return ToolRejectedError]
```
