use a2a_rs::{JsonRpcClient, Transport};
use a2a_rs::Message;

const FILE_AGENT: &str = "http://localhost:8080";
const DOCS_AGENT: &str = "http://localhost:8081";

/// Routes a task to the correct agent based on the CLI arguments.
///
/// Priority order:
///   1. Explicit flags (--file, --docs, --agent <url>) always win
///   2. Starts with `/`, `./`, or `~/`  → file-agent
///   3. Path exists on disk             → file-agent
///   4. Anything else                   → docs-agent
fn route(args: &[String]) -> Result<(String, String), String> {
    match args {
        // Explicit flags
        [_, flag, text] if flag == "--file"  => Ok((FILE_AGENT.into(), text.clone())),
        [_, flag, text] if flag == "--docs"  => Ok((DOCS_AGENT.into(), text.clone())),
        [_, flag, url, text] if flag == "--agent" => Ok((url.clone(), text.clone())),

        // Single argument: auto-dispatch
        [_, text] => Ok((autodetect(text), text.clone())),

        _ => Err(
            "Usage:\n  \
             a2a-cli <input>               → auto-dispatch\n  \
             a2a-cli --file  <path>        → file-agent on :8080\n  \
             a2a-cli --docs  <crate>       → docs-agent on :8081\n  \
             a2a-cli --agent <url> <text>  → arbitrary agent\n\n\
             Examples:\n  \
             a2a-cli Cargo.toml            → reads the file\n  \
             a2a-cli /etc/hosts            → reads the file\n  \
             a2a-cli tokio                 → searches crates.io\n  \
             a2a-cli serde async           → searches crates.io"
            .into()
        ),
    }
}

fn autodetect(input: &str) -> String {
    let looks_like_path = input.starts_with('/')
        || input.starts_with("./")
        || input.starts_with("~/");

    let exists_on_disk = std::path::Path::new(input).exists();

    if looks_like_path || exists_on_disk {
        FILE_AGENT.into()
    } else {
        DOCS_AGENT.into()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let (agent_url, task_text) = route(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let agent_name = if agent_url == FILE_AGENT {
        "file-agent"
    } else if agent_url == DOCS_AGENT {
        "docs-agent"
    } else {
        &agent_url
    };

    let message_id = uuid::Uuid::new_v4().to_string();
    let task_id    = uuid::Uuid::new_v4().to_string();

    let message = Message::user_text(task_text.clone(), message_id);

    eprintln!("[{agent_name}] → {task_text}");

    let client = JsonRpcClient::new(agent_url.clone());

    let task = client
        .send_task_message(&task_id, &message, None, None)
        .await
        .map_err(|e| format!("Error contacting {agent_url}: {e}"))?;

    let response_text = task
        .status
        .as_option()
        .and_then(|s| s.message.as_option())
        .and_then(|m| m.parts.iter().find_map(|p| p.get_text()).map(str::to_owned));

    match response_text {
        Some(text) => println!("{text}"),
        None => {
            let state = task
                .status
                .as_option()
                .map(|s| format!("{:?}", s.state))
                .unwrap_or_else(|| "unknown".into());
            eprintln!("No text in response. Task state: {state}");
        }
    }

    Ok(())
}
