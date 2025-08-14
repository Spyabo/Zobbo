//! ID utilities (ULIDs, short tokens).

use rand::{distributions::Alphanumeric, Rng};
use ulid::Ulid;

/// Generate a short room ID using ULID, truncated for readability.
pub fn new_room_id() -> String {
    let ulid = Ulid::new().to_string();
    // 26-char ULID, take first 10 for brevity. Collisions are extremely unlikely for MVP.
    ulid.chars().take(10).collect()
}

/// Generate a short join token (URL-safe alphanumeric).
pub fn new_join_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}
