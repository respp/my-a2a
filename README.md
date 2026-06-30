# my-a2a

Multi-agent CLI dispatcher built on the [A2A protocol](https://google.github.io/A2A/) using Rust.

Two specialized agents run as independent HTTP servers. The CLI detects what you're asking for and routes the task to the right one automatically — no flags needed.

## Agents

| Agent | Port | What it does |
|---|---|---|
| `file-agent` | 8080 | Reads a local file and returns its content |
| `docs-agent` | 8081 | Searches crates.io and returns top 5 results |

## How to run

```bash
# Build everything once
cargo build -p file-agent -p docs-agent -p a2a-cli

# Start agents (each in its own terminal)
./target/debug/file-agent
./target/debug/docs-agent

# Use the CLI — it routes automatically
./target/debug/a2a-cli /path/to/file.txt    # → file-agent
./target/debug/a2a-cli Cargo.toml           # → file-agent (file exists locally)
./target/debug/a2a-cli tokio                # → docs-agent
./target/debug/a2a-cli serde async          # → docs-agent

# Or with explicit flags
./target/debug/a2a-cli --file /path/to/file
./target/debug/a2a-cli --docs serde
./target/debug/a2a-cli --agent http://localhost:9000 "some task"
```

## How the dispatcher works

```
input: "/etc/hosts"     → starts with /        → file-agent :8080
input: "Cargo.toml"    → file exists on disk   → file-agent :8080
input: "tokio"         → none of the above     → docs-agent :8081
```

The rule is simple: if it looks like a file path or the file actually exists, it goes to `file-agent`. Everything else goes to `docs-agent`.

## Architecture

Every agent speaks the same A2A protocol (JSON-RPC 2.0 over HTTP). The CLI sends a `SendMessage` request and waits for a `Completed` task with the response. Neither the CLI nor the agents know anything about each other's internals — they only share the protocol contract.

```
CLI  ──SendMessage──►  file-agent :8080  (reads file, returns content)
     ──SendMessage──►  docs-agent :8081  (calls crates.io, returns results)
```

Each agent also exposes its capabilities at `GET /.well-known/agent-card.json`, which any A2A-compatible orchestrator can use for discovery.

### Code structure

Inside each agent there are three layers:

```
Responder           — your business logic (read file / call crates.io)
    ↓ wrapped by
ResponderMessageHandler  — handles the task lifecycle (Submitted → Working → Completed)
    ↓ wired into
JsonRpcAdapter      — HTTP routing (JSON-RPC 2.0 + REST endpoints)
```

Only the `Responder` layer contains custom code. The rest is provided by `EmilLindfors/a2a-rs`.

## Running tests

```bash
cargo test -p file-agent    # 3 unit tests, no server needed
cargo test -p docs-agent    # 3 unit tests, no network needed
```

The `file-agent` tests exercise `FileResponder` directly — no HTTP server, no Axum, just the business logic function. They cover: reading a file successfully, missing file (IO error), and empty message (invalid params).

The `docs-agent` injects `CrateSearch` as a trait so tests use a `FakeCrateSearch` — no real HTTP calls. Tests cover: empty message (invalid params), successful search result, and query passthrough verification.

## Wire compatibility with a2aproject/a2a-rs

The [official A2A SDK from Google](https://github.com/a2aproject/a2a-rs) and `EmilLindfors/a2a-rs` are **not wire-compatible**. Here's what differs:

| | EmilLindfors (this project) | a2aproject (official SDK) |
|---|---|---|
| JSON-RPC method | `SendMessage` | `message/send` |
| Role field | `"ROLE_USER"` | `"user"` |
| Part format | `{"text": "..."}` | `{"type": "text", "text": "..."}` |

EmilLindfors uses Protocol Buffers' JSON encoding (ProtoJSON), where enum values are serialized as `SCREAMING_SNAKE_CASE` strings. The official A2A spec uses standard JSON with lowercase strings and an explicit `type` field on parts.

Tested live:

```bash
# message/send → -32601 Method not found
# SendMessage with "role": "user" → -32602 Invalid parameters: expected a known enum variant name
```

The two cannot talk to each other without a translation layer.

## Crates used

- [`EmilLindfors/a2a-rs`](https://github.com/EmilLindfors/a2a-rs) — A2A server (`jsonrpc-server` feature) and client (`jsonrpc-client` feature)
- [`reqwest`](https://crates.io/crates/reqwest) — HTTP client for crates.io API in `docs-agent`
- [`serde`](https://crates.io/crates/serde) — JSON deserialization for crates.io responses
