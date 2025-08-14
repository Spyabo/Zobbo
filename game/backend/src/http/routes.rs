//! HTTP routes: lobby, create/join room, health, template rendering endpoints.

use askama::Template;
use axum::{extract::{Path, Query, State}, response::{IntoResponse, Redirect}, Form};
use serde::Deserialize;
use axum::http::StatusCode;

use crate::room::manager::{RoomError, RoomManager};

#[derive(Clone)]
pub struct AppState {
    pub rooms: RoomManager,
}

#[derive(Template)]
#[template(path = "room.html")]
struct RoomTemplate {
    room_id: String,
    has_invite: bool,
    invite_token: String,
}

pub async fn create_room(State(state): State<AppState>) -> impl IntoResponse {
    let created = state.rooms.create_room();
    let redirect_to = format!("/rooms/{}/view?token={}", created.id, created.creator_token);
    Redirect::to(&redirect_to)
}

#[derive(Deserialize)]
pub struct JoinForm {
    pub token: String,
}

pub async fn join_room(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Form(JoinForm { token }): Form<JoinForm>,
) -> impl IntoResponse {
    match state.rooms.join_room(&id, &token) {
        Ok(()) => Redirect::to(&format!("/rooms/{}/view?token={}", id, token)).into_response(),
        Err(RoomError::NotFound) => (StatusCode::NOT_FOUND, "room not found").into_response(),
        Err(RoomError::InvalidToken) => (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
        Err(RoomError::Full) => (StatusCode::CONFLICT, "room full").into_response(),
    }
}

#[derive(Deserialize)]
pub struct ViewQuery { pub token: String }

pub async fn view_room(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Query(ViewQuery { token }): Query<ViewQuery>,
) -> impl IntoResponse {
    // Validate visibility: room exists and token is one of the room's tokens
    let ok = state.rooms.has_token(&id, &token);
    if !ok {
        return (StatusCode::UNAUTHORIZED, "invalid room or token").into_response();
    }
    // Try to compute the other token for convenience
    let invite = state.rooms.other_token(&id, &token);
    let (has_invite, invite_token) = match invite {
        Some(t) => (true, t),
        None => (false, String::new()),
    };
    RoomTemplate { room_id: id, has_invite, invite_token }.into_response()
}
