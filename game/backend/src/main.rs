use axum::{routing::{get, post}, Router};
use axum::response::IntoResponse;
use tower_http::services::ServeDir;
use askama::Template;
use std::net::SocketAddr;
use std::sync::Arc;

mod config;
mod http;
mod room;
mod util;
mod ws;

use crate::http::routes::{self, AppState};
use crate::room::manager::RoomManager;

#[derive(Template)]
#[template(path = "lobby.html")]
struct LobbyTemplate;

async fn lobby() -> impl IntoResponse { LobbyTemplate }

async fn healthz() -> &'static str { "ok" }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState { rooms: Arc::new(RoomManager::new()) };

    let app = Router::new()
        .route("/", get(lobby))
        .route("/healthz", get(healthz))
        .route("/rooms", post(routes::create_room))
        .route("/rooms/:id/join", post(routes::join_room))
        .route("/rooms/:id/view", get(routes::view_room))
        .route("/ws", get(ws::connection::ws_handler))
        // Serve static assets from the frontend directory
        .nest_service("/static", ServeDir::new(config::static_dir()))
        .with_state(state);

    let addr: SocketAddr = config::server_addr();
    tracing::info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
