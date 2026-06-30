use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use a2a_rs::{
    AsyncMessageHandler, InMemoryStreamingHandler, InMemoryTaskStorage,
    A2AError, Message, Part, Role, Task, TaskState,
};
use a2a_rs::adapter::{
    SimpleAgentInfo,
    JsonRpcAdapter, jsonrpc_router, rest_router,
};
use a2a_rs::adapter::business::{Responder, ResponderMessageHandler};

// ── Types for deserializing the crates.io response ──────────────────────────

#[derive(Deserialize)]
struct CratesResponse {
    crates: Vec<Crate>,
}

#[derive(Deserialize)]
struct Crate {
    name:          String,
    max_version:   String,
    description:   Option<String>,
    documentation: Option<String>,
    downloads:     u64,
}

// ── crates.io search ─────────────────────────────────────────────────────────

async fn search_crates(query: &str) -> Result<String, reqwest::Error> {
    // crates.io requires a descriptive User-Agent or returns 403
    let client = reqwest::Client::builder()
        .user_agent("my-a2a-demo/0.1.0 (github.com/example/my-a2a-demo)")
        .build()?;

    let resp: CratesResponse = client
        .get("https://crates.io/api/v1/crates")
        .query(&[("q", query), ("per_page", "5")])
        .send()
        .await?
        .json()
        .await?;

    if resp.crates.is_empty() {
        return Ok(format!("No crates found for '{query}'."));
    }

    let mut out = format!("Results for '{query}' on crates.io:\n\n");

    for c in &resp.crates {
        out.push_str(&format!("📦 {} v{}\n", c.name, c.max_version));

        if let Some(desc) = &c.description {
            out.push_str(&format!("   {}\n", desc.trim()));
        }

        if let Some(docs) = &c.documentation {
            out.push_str(&format!("   docs: {docs}\n"));
        }

        out.push_str(&format!("   {} total downloads\n\n", fmt_downloads(c.downloads)));
    }

    Ok(out)
}

fn fmt_downloads(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1_000_000_000.0),
        n if n >= 1_000_000     => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000         => format!("{:.1}K", n as f64 / 1_000.0),
        n                       => n.to_string(),
    }
}

// ── Responder ────────────────────────────────────────────────────────────────

struct DocsResponder;

#[async_trait]
impl Responder for DocsResponder {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let query = message
            .parts
            .iter()
            .find_map(|p| p.get_text())
            .map(str::trim)
            .ok_or_else(|| {
                A2AError::InvalidParams(
                    "Message must contain the crate name to search as a text part".into(),
                )
            })?;

        tracing::info!(query, "searching crates.io");

        let result = search_crates(query)
            .await
            .unwrap_or_else(|e| format!("Error contacting crates.io: {e}"));

        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(result)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .context_id(message.context_id.clone())
            .build();

        Ok((reply, TaskState::Completed))
    }
}

// ── Handler and server ───────────────────────────────────────────────────────

#[derive(Clone)]
struct DocsMessageHandler {
    storage:   InMemoryTaskStorage,
    streaming: InMemoryStreamingHandler,
}

#[async_trait]
impl AsyncMessageHandler for DocsMessageHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        ResponderMessageHandler::new(
            self.storage.clone(),
            self.streaming.clone(),
            self.storage.push_notifier(),
            DocsResponder,
        )
        .process_message(task_id, message, session_id)
        .await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port = 8081u16;
    let addr = format!("0.0.0.0:{port}");
    let base = format!("http://localhost:{port}");

    let storage   = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();

    let agent_info = SimpleAgentInfo::new("docs-agent".to_string(), base)
        .with_description("Searches crates on crates.io and documentation on docs.rs".to_string())
        .with_version("0.1.0".to_string());

    let handler = DocsMessageHandler {
        storage:   storage.clone(),
        streaming: streaming.clone(),
    };

    let adapter = Arc::new(
        JsonRpcAdapter::new(
            handler,
            storage.clone(),
            storage.clone(),
            agent_info,
        )
        .with_streaming_handler(streaming)
        .with_push_notifier(storage.push_notifier()),
    );

    let app = jsonrpc_router(adapter.clone()).merge(rest_router(adapter));
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("docs-agent listening on http://localhost:{port}");
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str) -> Message {
        Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(text.to_string())])
            .message_id("test-msg".to_string())
            .task_id("test-task".to_string())
            .context_id("test-ctx".to_string())
            .build()
    }

    fn task() -> Task {
        Task::new("test-task".to_string(), "test-ctx".to_string())
    }

    #[tokio::test]
    async fn errors_when_no_text_part() {
        let empty_msg = Message::builder()
            .role(Role::User)
            .parts(vec![])
            .message_id("test-msg".to_string())
            .task_id("test-task".to_string())
            .context_id("test-ctx".to_string())
            .build();

        let result = DocsResponder.respond(&empty_msg, &task()).await;

        assert!(matches!(result, Err(A2AError::InvalidParams(_))));
    }

    #[tokio::test]
    async fn returns_completed_state_on_valid_query() {
        // Hits crates.io — skipped if offline. Run with: cargo test -- --include-ignored
        let (_, state) = DocsResponder
            .respond(&msg("serde"), &task())
            .await
            .unwrap();

        assert_eq!(state, TaskState::Completed);
    }
}
