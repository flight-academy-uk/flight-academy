//! Thin entrypoint per ADR-005 §D — config, build app, serve. Subcommands:
//!
//! * `serve` (default) — bind and serve the HTTP API.
//! * `emit-spec` — write the assembled OpenAPI document as pretty JSON to
//!   stdout. CI redirects this to `docs/api/openapi.json` and diffs against
//!   the committed copy per ADR-006 §A (path per ADR-018).
//! * `migrate` — runs database migrations against DATABASE_URL. Same
//!   subcommand is invoked by the hosted Kubernetes Job via Flagger's
//!   pre-rollout webhook (ADR-003 §C) and by the self-host install script
//!   (ADR-002 §I); the binary and the migration set are identical. Lands
//!   with the DB foundation commit.

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "flight-academy", about = "Flight Academy API", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the HTTP API (default if no subcommand given).
    Serve,
    /// Emit the OpenAPI document as pretty JSON to stdout
    /// (ADR-006 §A; format per ADR-018).
    EmitSpec,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    match Cli::parse().command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
        Command::EmitSpec => emit_spec(),
    }
}

fn emit_spec() -> std::io::Result<()> {
    let openapi = flight_academy_api::openapi();
    let json = openapi
        .to_pretty_json()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    println!("{json}");
    Ok(())
}

async fn serve() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    // Secure-by-default per ADR-004 §F / ADR-016 §E: loopback-only for
    // native `cargo run`. Containers (Dockerfile / Helm / docker-compose)
    // override with `BIND_ADDR=0.0.0.0:8080` since the container's network
    // namespace is the security boundary there.
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "flight-academy listening");
    axum::serve(listener, flight_academy_api::app()).await
}
