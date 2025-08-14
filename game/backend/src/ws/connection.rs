//! WebSocket connection lifecycle management.

use axum::{extract::{Query, State}, response::IntoResponse};
use axum::http::StatusCode;
use axum::extract::ws::{WebSocketUpgrade, WebSocket, Message};
use serde::Deserialize;

use crate::http::routes::AppState;

#[derive(Deserialize)]
pub struct WsParams {
    pub room_id: String,
    pub token: String,
}

pub async fn ws_handler(
    State(state): State<AppState>,
    Query(WsParams { room_id, token }): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !state.rooms.has_token(&room_id, &token) {
        return (StatusCode::UNAUTHORIZED, "invalid room or token").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, room_id, token))
}

async fn handle_socket(mut socket: WebSocket, room_id: String, token: String) {
    let _ = socket
        .send(Message::Text(format!("welcome to room {}", room_id)))
        .await;
    // Simple echo/read loop placeholder
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let _ = socket.send(Message::Text(format!("echo: {}", text))).await;
            }
            Message::Binary(bin) => {
                let _ = socket.send(Message::Binary(bin)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    tracing::debug!(%room_id, %token, "ws closed");
}
