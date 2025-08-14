//! Tracing and metrics initialization hooks.

use tracing_subscriber::{fmt, EnvFilter, prelude::*};

/// Initialize global tracing subscriber with env filter.
///
/// Use RUST_LOG to configure, e.g.:
/// RUST_LOG=debug,axum=info,tower_http=info
pub fn init() {
    let fmt_layer = fmt::layer()
        .with_target(true);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,axum=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}
