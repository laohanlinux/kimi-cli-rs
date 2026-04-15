# Kimi CLI Architecture Overview

> Source analyzed: `/Users/rg/Projects/kimi-cli/src/kimi_cli`

## 1. High-Level Component Diagram

```mermaid
graph TB
    subgraph Entry["Entry Points"]
        E1["__main__.py<br/>python -m kimi_cli"]
        E2["cli/__main__.py<br/>python -m kimi_cli.cli"]
        E3["web/runner/worker.py<br/>Web Worker Subprocess"]
    end

    subgraph CLI["CLI Layer (cli/)"]
        C1["Typer CLI App<br/>cli/__init__.py:40"]
        C2["LazySubcommandGroup<br/>cli/_lazy_group.py"]
        C3["Subcommands: login, web, vis, mcp, plugin, export, info"]
    end

    subgraph App["Application Orchestrator (app.py)"]
        A0["KimiCLI<br/>Factory + Lifecycle"]
    end

    subgraph Core["Core Runtime (soul/)"]
        S1["KimiSoul<br/>soul/kimisoul.py:117"]
        S2["Agent<br/>soul/agent.py:382"]
        S3["Context<br/>soul/context.py:20"]
        S4["KimiToolset<br/>soul/toolset.py:93"]
        S5["Runtime<br/>soul/agent.py:185"]
    end

    subgraph WireSys["Wire Protocol (wire/)"]
        W1["Wire<br/>wire/__init__.py:18"]
        W2["WireServer<br/>wire/server.py:82"]
        W3["RootWireHub<br/>wire/root_hub.py:8"]
        W4["JSON-RPC Bridge<br/>wire/jsonrpc.py"]
    end

    subgraph Tools["Tools (tools/)"]
        T1["File Tools<br/>read, write, replace, glob, grep"]
        T2["Web Tools<br/>search, fetch"]
        T3["Shell Tool<br/>bash/powershell"]
        T4["Special Tools<br/>ask_user, plan_mode, background, think, todo, dmail"]
        T5["MCP Tools<br/>External MCP servers"]
    end

    subgraph UILayer["UI Layer (ui/)"]
        U1["Shell UI<br/>ui/shell/"]
        U2["Print UI<br/>ui/print/"]
        U3["ACP UI<br/>ui/acp/"]
    end

    subgraph Ext["Extensions"]
        X1["Background Tasks<br/>background/"]
        X2["Subagents<br/>subagents/"]
        X3["Notifications<br/>notifications/"]
        X4["Approval Runtime<br/>approval_runtime/"]
        X5["Hooks<br/>hooks/"]
        X6["OAuth / Auth<br/>auth/"]
    end

    subgraph WebACP["Servers"]
        Srv1["Web API<br/>web/app.py"]
        Srv2["Vis API<br/>vis/app.py"]
        Srv3["ACP Server<br/>acp/server.py"]
    end

    E1 --> C1
    E2 --> C1
    C1 --> A0
    A0 --> S5
    S5 --> S2
    S5 --> S1
    S1 --> S3
    S1 --> S4
    S2 --> S4
    S1 --> W1
    W1 --> W2
    W2 --> W4
    S4 --> T1
    S4 --> T2
    S4 --> T3
    S4 --> T4
    S4 --> T5
    A0 --> U1
    A0 --> U2
    A0 --> U3
    S5 --> X1
    S5 --> X2
    S5 --> X3
    S5 --> X4
    S5 --> X5
    S5 --> X6
    X1 --> W3
    X2 --> W3
    X3 --> W3
    X4 --> W3
    Srv1 --> A0
    Srv3 --> A0
```

## 2. Layered Architecture

```mermaid
flowchart TB
    subgraph Layer4["Presentation Layer"]
        direction TB
        Shell["Interactive Shell (prompt_toolkit + Rich)"]
        Print["Print Mode (stdin/stdout)"]
        WebUI["Web UI (FastAPI + WebSocket)"]
        VisUI["Vis UI (FastAPI static SPA)"]
        ACPCli["ACP Client / MCP Host"]
    end

    subgraph Layer3["Application Layer"]
        direction TB
        KimiCLI["KimiCLI (app.py)"]
        SessionMgr["Session Manager (session.py)"]
        ConfigMgr["Config Manager (config.py)"]
    end

    subgraph Layer2["Domain Layer (Agent Core)"]
        direction TB
        KimiSoul["KimiSoul (soul/kimisoul.py)"]
        Agent["Agent (soul/agent.py)"]
        Context["Context (soul/context.py)"]
        Toolset["KimiToolset (soul/toolset.py)"]
    end

    subgraph Layer1["Infrastructure Layer"]
        direction TB
        Wire["Wire Protocol"]
        LLM["LLM Provider (kosong)"]
        Kaos["Kaos FS / Process"]
        OAuth["OAuth Manager"]
        Store["Disk Stores (metadata, state, wire)"]
    end

    Layer4 <-->|WireMessage| Layer3
    Layer3 -->|run_soul()| Layer2
    Layer2 -->|ChatProvider| Layer1
    Layer2 -->|Tool Calls| Layer1
    Layer3 -->|Session/Config I/O| Layer1
```

## 3. Key Directory Responsibilities

| Directory | Responsibility |
|-----------|----------------|
| `cli/` | Typer-based command-line interface, lazy-loaded subcommands |
| `app.py` | `KimiCLI` factory and run-mode dispatch (shell/print/acp/wire) |
| `session.py` | Session lifecycle: create, find, list, continue, delete |
| `soul/` | Agent execution engine: KimiSoul, context, toolset, runtime |
| `wire/` | Internal message protocol between soul and UI |
| `tools/` | Tool implementations: file, shell, web, special tools |
| `ui/` | UI implementations: shell, print, acp |
| `background/` | Background bash/agent task management |
| `subagents/` | Subagent registry, builder, runner, store |
| `notifications/` | Notification publish/delivery/ack system |
| `approval_runtime/` | Approval request/response routing |
| `hooks/` | Event-driven shell-command hooks |
| `auth/` | OAuth device flow and token refresh |
| `web/` | FastAPI web UI backend with worker subprocess model |
| `vis/` | FastAPI visualization backend for session tracing |
| `acp/` | ACP/MCP server exposing Kimi as an agent |
| `utils/` | Shared utilities: logging, paths, export, queues, broadcast |
