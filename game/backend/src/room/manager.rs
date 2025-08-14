//! Registry of rooms and task orchestration.

use std::time::{Duration, SystemTime};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::util::id::{new_join_token, new_room_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub tokens: Vec<String>, // simple list for MVP (creator + invite)
    pub players: usize,
    pub created_at: SystemTime,
}

impl Room {
    fn new() -> (Self, String, String) {
        let id = new_room_id();
        let creator = new_join_token();
        let invite = new_join_token();
        let room = Room {
            id: id.clone(),
            tokens: vec![creator.clone(), invite.clone()],
            players: 0,
            created_at: SystemTime::now(),
        };
        (room, creator, invite)
    }

    fn has_token(&self, token: &str) -> bool {
        self.tokens.iter().any(|t| t == token)
    }
}

#[derive(Clone, Default)]
pub struct RoomManager {
    rooms: DashMap<String, Room>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedRoom {
    pub id: String,
    pub creator_token: String,
    pub invite_token: String,
}

#[derive(thiserror::Error, Debug)]
pub enum RoomError {
    #[error("room not found")]
    NotFound,
    #[error("invalid token")]
    InvalidToken,
    #[error("room full")]
    Full,
}

impl RoomManager {
    pub fn new() -> Self { Self { rooms: DashMap::new() } }

    pub fn create_room(&self) -> CreatedRoom {
        let (room, creator, invite) = Room::new();
        let id = room.id.clone();
        self.rooms.insert(id.clone(), room);
        CreatedRoom { id, creator_token: creator, invite_token: invite }
    }

    pub fn join_room(&self, id: &str, token: &str) -> Result<(), RoomError> {
        let mut entry = self.rooms.get_mut(id).ok_or(RoomError::NotFound)?;
        if !entry.has_token(token) { return Err(RoomError::InvalidToken); }
        if entry.players >= 2 { return Err(RoomError::Full); }
        entry.players += 1;
        Ok(())
    }

    pub fn has_token(&self, id: &str, token: &str) -> bool {
        self.rooms.get(id).map(|r| r.has_token(token)).unwrap_or(false)
    }

    /// Returns the other token in the room that is not `token`, if any.
    pub fn other_token(&self, id: &str, token: &str) -> Option<String> {
        self.rooms
            .get(id)
            .and_then(|r| r.tokens.iter().find(|t| *t != token).cloned())
    }

    #[allow(dead_code)]
    pub fn prune_old(&self, max_age: Duration) {
        let now = SystemTime::now();
        self.rooms.retain(|_, r| now.duration_since(r.created_at).unwrap_or_default() < max_age);
    }
}
