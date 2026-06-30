# my-a2a

Multi-agent CLI dispatcher built on the [A2A protocol](https://google.github.io/A2A/) using Rust.

## Agents

| Agent | Port | What it does |
|---|---|---|
| `file-agent` | 8080 | Reads local files and returns their content |
| `docs-agent` | 8081 | Searches crates.io and returns results |

## Usage

```bash
# Start agents (each in its own terminal)
cargo run -p file-agent
cargo run -p docs-agent

# Auto-dispatch: CLI decides which agent based on the input
cargo run -p a2a-cli -- /path/to/file.txt   # → file-agent
cargo run -p a2a-cli -- Cargo.toml          # → file-agent (file exists locally)
cargo run -p a2a-cli -- tokio               # → docs-agent
cargo run -p a2a-cli -- serde async         # → docs-agent

# Explicit routing
cargo run -p a2a-cli -- --file /path/to/file
cargo run -p a2a-cli -- --docs serde
cargo run -p a2a-cli -- --agent http://localhost:9000 "task text"
```

## How it works

Every agent speaks the same A2A protocol (JSON-RPC 2.0 over HTTP). The CLI sends a `SendMessage` request and waits for a `Completed` task with the response.

The dispatcher detects file paths (starts with `/`, `./`, `~/`, or the file exists on disk) and routes to `file-agent`. Everything else goes to `docs-agent`.

Each agent exposes its capabilities at `/.well-known/agent-card.json`.

## Crates used

- [`EmilLindfors/a2a-rs`](https://github.com/EmilLindfors/a2a-rs) — A2A server (`jsonrpc-server`) and client (`jsonrpc-client`) implementation
- [`reqwest`](https://crates.io/crates/reqwest) — HTTP client for crates.io API calls
