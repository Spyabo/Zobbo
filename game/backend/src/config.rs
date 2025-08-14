//! Configuration utilities (ports, CORS, env vars)

use std::{env, net::{Ipv4Addr, SocketAddr}};
use std::path::{Path, PathBuf};

/// Socket address to bind the server to.
///
/// Reads the `PORT` env var (Fly.io) or defaults to 8080, binds to 0.0.0.0.
pub fn server_addr() -> SocketAddr {
    let port = env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8080);
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))
}

/// Resolve the static directory path used by the server.
/// Order:
/// 1) STATIC_DIR env var
/// 2) ./game/frontend/static (container runtime layout)
/// 3) ../frontend/static (local dev from backend dir)
pub fn static_dir() -> PathBuf {
    if let Ok(p) = env::var("STATIC_DIR") {
        return PathBuf::from(p);
    }
    let p1 = Path::new("./game/frontend/static");
    if p1.exists() { return p1.to_path_buf(); }
    PathBuf::from("../frontend/static")
}
