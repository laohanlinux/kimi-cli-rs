# Wire Protocol Architecture

## 1. Wire Channel Structure

```mermaid
graph TB
    subgraph Wire["Wire (wire/__init__.py:18)"]
        SS[WireSoulSide]
        UI[WireUISide]
        Raw[Raw BroadcastQueue]
        Merge[Merged BroadcastQueue]
        Rec[_WireRecorder → wire.jsonl]
    end

    Soul["KimiSoul"] --send(msg)--> SS
    SS --publish--> Raw
    SS --merge_buffer.flush--> Merge
    Raw --> UI
    Merge --> UI
    Merge --> Rec
    UI --> Live["_LiveView / _PromptLiveView"]
    UI --> Print["Print.visualize"]
    UI --> WireSvr["WireServer"]
```

## 2. Wire Message Type Hierarchy

```mermaid
classDiagram
    class WireMessage
    <<union>> WireMessage

    class Event
    <<union>> Event
    Event : TurnBegin
    Event : StepBegin
    Event : StepInterrupted
    Event : TurnEnd
    Event : CompactionBegin
    Event : CompactionEnd
    Event : StatusUpdate
    Event : Notification
    Event : PlanDisplay
    Event : BtwBegin
    Event : BtwEnd
    Event : SubagentEvent
    Event : HookTriggered
    Event : HookResolved
    Event : MCPLoadingBegin
    Event : MCPLoadingEnd

    class ContentPart
    <<union>> ContentPart
    ContentPart : TextPart
    ContentPart : ThinkPart
    ContentPart : ImageURLPart
    ContentPart : AudioURLPart
    ContentPart : VideoURLPart

    class Tooling
    <<union>> Tooling
    Tooling : ToolCall
    Tooling : ToolCallPart
    Tooling : ToolResult
    Tooling : ApprovalResponse

    class Request
    <<union>> Request
    Request : ApprovalRequest
    Request : QuestionRequest
    Request : ToolCallRequest
    Request : HookRequest

    WireMessage <|-- Event
    WireMessage <|-- ContentPart
    WireMessage <|-- Tooling
    WireMessage <|-- Request
```

## 3. JSON-RPC Bridge Message Flow

```mermaid
sequenceDiagram
    participant Client as JSON-RPC Client
    participant Read as _read_loop
    participant Dispatch as _dispatch_msg
    participant Soul as KimiSoul
    participant Stream as _stream_wire_messages
    participant Write as _write_loop

    Client->>Read: {"jsonrpc":"2.0","method":"initialize",...}
    Read->>Dispatch: initialize(params)
    Dispatch->>Dispatch: Register external tools
    Dispatch-->>Write: JSONRPCSuccessResponse
    Write-->>Client: stdout line

    Client->>Read: {"jsonrpc":"2.0","method":"prompt",...}
    Read->>Dispatch: prompt(params)
    Dispatch->>Soul: run_soul(soul, input, _ui_loop_fn)
    Soul->>Stream: Yield WireMessages
    Stream->>Stream: _request_approval
    Stream->>Write: JSONRPCRequestMessage
    Write-->>Client: ApprovalRequest
    Client->>Read: Response message
    Read->>Dispatch: _handle_response
    Dispatch->>Dispatch: ApprovalRequest.resolve
    Dispatch-->>Soul: Unblocks tool execution

    Soul->>Stream: TextPart chunks
    Stream->>Write: JSONRPCEventMessage
    Write-->>Client: Streaming text
```

## 4. RootWireHub Broadcast Model

```mermaid
graph LR
    subgraph RootHub["RootWireHub"]
        BQ[BroadcastQueue]
    end

    AR[ApprovalRuntime] --ApprovalRequest--> BQ
    AR[ApprovalRuntime] --ApprovalResponse--> BQ
    BG[Background Agents] --SubagentEvent--> BQ
    Notif[NotificationManager] --Notification--> BQ

    BQ --> ShellUI[Shell UI]
    BQ --> WebSocket[WebSocket Client]
    BQ --> WireSvr[WireServer]
```

## 5. WireServer Inbound Methods

| JSON-RPC Method | Handler | Purpose |
|-----------------|---------|---------|
| `initialize` | `_handle_initialize` | Register external tools, hooks, capabilities |
| `prompt` | `_handle_prompt` | Start a new soul turn |
| `steer` | `_handle_steer` | Inject follow-up input into running turn |
| `replay` | `_handle_replay` | Replay persisted wire file messages |
| `set_plan_mode` | `_handle_set_plan_mode` | Toggle plan mode |
| `cancel` | `_handle_cancel` | Cancel current turn |
| *(response)* | `_handle_response` | Resolve pending approval/question/tool/hook requests |

## 6. Wire File Persistence Format

```
~/.kimi/sessions/<hash>/<session_id>/wire.jsonl

{"v":1,"created_at":<timestamp>}
{"type":"TurnBegin","payload":{...}}
{"type":"ContentPart.TextPart","payload":{"text":"Hello"}}
{"type":"ToolCall","payload":{"tool_call_id":"...",...}}
{"type":"ToolResult","payload":{"tool_call_id":"...",...}}
{"type":"TurnEnd","payload":{"stop_reason":"no_tool_calls"}}
```
