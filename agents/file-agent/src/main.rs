use std::sync::Arc;

use async_trait::async_trait;

use a2a_rs::{
    AsyncMessageHandler, InMemoryStreamingHandler, InMemoryTaskStorage,
    A2AError, Message, Part, Role, Task, TaskState,
};
use a2a_rs::adapter::{
    SimpleAgentInfo,
    JsonRpcAdapter, jsonrpc_router, rest_router,
};
use a2a_rs::adapter::business::{Responder, ResponderMessageHandler};

// ── Lógica de negocio ────────────────────────────────────────────────────────

// FileResponder: recibe el mensaje del usuario y devuelve el contenido del archivo
struct FileResponder;

#[async_trait]
impl Responder for FileResponder {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        // El primer part de texto contiene el path del archivo
        let path = message
            .parts
            .iter()
            .find_map(|p| p.get_text())
            .map(str::trim)
            .ok_or_else(|| A2AError::InvalidParams(
                "El mensaje debe contener el path del archivo como text part".into(),
            ))?;

        tracing::info!(path, "leyendo archivo");

        // A2AError implementa From<std::io::Error>, así que ? funciona directo
        let content = tokio::fs::read_to_string(path).await?;

        tracing::info!(path, bytes = content.len(), "archivo leído");

        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(content)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .context_id(message.context_id.clone())
            .build();

        Ok((reply, TaskState::Completed))
    }
}

// ── Handler del agente ───────────────────────────────────────────────────────

// FileMessageHandler coordina el ciclo de vida del task + delega a FileResponder
#[derive(Clone)]
struct FileMessageHandler {
    storage:   InMemoryTaskStorage,
    streaming: InMemoryStreamingHandler,
}

#[async_trait]
impl AsyncMessageHandler for FileMessageHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // ResponderMessageHandler maneja:
        //   1. create() del task en storage
        //   2. update_status(Working)
        //   3. llama a FileResponder::respond()
        //   4. update_status(Completed) con la respuesta
        //   5. broadcast a suscriptores SSE
        ResponderMessageHandler::new(
            self.storage.clone(),
            self.streaming.clone(),
            self.storage.push_notifier(),
            FileResponder,
        )
        .process_message(task_id, message, session_id)
        .await
    }
}

// ── Bootstrap del servidor ───────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port = 8080u16;
    let addr = format!("0.0.0.0:{port}");
    let base = format!("http://localhost:{port}");

    // Storage compartido: todos los clones apuntan al mismo Arc interno
    let storage   = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();

    let agent_info = SimpleAgentInfo::new("file-agent".to_string(), base.clone())
        .with_description("Lee archivos locales y devuelve su contenido".to_string())
        .with_version("0.1.0".to_string());

    let file_handler = FileMessageHandler {
        storage:   storage.clone(),
        streaming: streaming.clone(),
    };

    // JsonRpcAdapter::new separa:
    //   - message_handler  → process_message (lógica de negocio)
    //   - tasks            → GetTask, CancelTask, ListTasks
    //   - notification_mgr → push config endpoints
    let adapter = Arc::new(
        JsonRpcAdapter::new(
            file_handler,
            storage.clone(),  // AsyncTaskLifecycle + AsyncTaskQuery
            storage.clone(),  // AsyncNotificationManager
            agent_info,
        )
        .with_streaming_handler(streaming)            // habilita SSE
        .with_push_notifier(storage.push_notifier()), // habilita push notifications
    );

    let app = jsonrpc_router(adapter.clone())
        .merge(rest_router(adapter));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("file-agent escuchando en http://localhost:{port}");
    tracing::info!("  JSON-RPC:   POST http://localhost:{port}/");
    tracing::info!("  Agent card: GET  http://localhost:{port}/.well-known/agent-card.json");

    axum::serve(listener, app).await?;

    Ok(())
}
