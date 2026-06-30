use a2a_rs::{JsonRpcClient, Transport};
use a2a_rs::Message;

const FILE_AGENT: &str = "http://localhost:8080";
const DOCS_AGENT: &str = "http://localhost:8081";

/// Decide a qué agente mandar la tarea según cómo se ve el input.
///
/// Reglas, en orden:
///   1. Flags explícitos (--file, --docs, --agent <url>) → siempre ganan
///   2. Empieza con `/`, `./` o `~/`                     → file-agent
///   3. Es un path que existe en disco                   → file-agent
///   4. Cualquier otra cosa                              → docs-agent
fn route(args: &[String]) -> Result<(String, String), String> {
    match args {
        // Flags explícitos
        [_, flag, text] if flag == "--file"  => Ok((FILE_AGENT.into(), text.clone())),
        [_, flag, text] if flag == "--docs"  => Ok((DOCS_AGENT.into(), text.clone())),
        [_, flag, url, text] if flag == "--agent" => Ok((url.clone(), text.clone())),

        // Un solo argumento: autodispatch
        [_, text] => Ok((autodetect(text), text.clone())),

        _ => Err(
            "Uso:\n  \
             a2a-cli <input>               → despacha automáticamente\n  \
             a2a-cli --file  <path>        → file-agent en :8080\n  \
             a2a-cli --docs  <crate>       → docs-agent en :8081\n  \
             a2a-cli --agent <url> <texto> → agente arbitrario\n\n\
             Ejemplos:\n  \
             a2a-cli Cargo.toml            → lee el archivo\n  \
             a2a-cli /etc/hosts            → lee el archivo\n  \
             a2a-cli tokio                 → busca en crates.io\n  \
             a2a-cli serde async           → busca en crates.io"
            .into()
        ),
    }
}

fn autodetect(input: &str) -> String {
    // Parece un path si empieza con separadores de directorio
    let looks_like_path = input.starts_with('/')
        || input.starts_with("./")
        || input.starts_with("~/");

    // O si el archivo realmente existe en disco
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
        .map_err(|e| format!("Error contactando {agent_url}: {e}"))?;

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
                .unwrap_or_else(|| "desconocido".into());
            eprintln!("Sin respuesta de texto. Estado: {state}");
        }
    }

    Ok(())
}
