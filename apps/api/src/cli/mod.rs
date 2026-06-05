//! CLI surface for the `flight-academy` binary — subcommand parsing and
//! dispatch. Lives separately from the HTTP library (lib.rs and its
//! modules) per ADR-005 §D's "thin entrypoint" guidance: main.rs is just
//! the wrapper that calls [`run`].
//!
//! Subcommands:
//!
//! * `serve` (default) — bind and serve the HTTP API.
//! * `emit-spec` — write the assembled OpenAPI document as pretty JSON to
//!   stdout. CI redirects this to `docs/api/openapi.json` and diffs against
//!   the committed copy per ADR-006 §A (format per ADR-018).
//! * `migrate` — apply pending database migrations against `DATABASE_URL`.
//!   Same subcommand is invoked by the hosted Kubernetes Job via Flagger's
//!   pre-rollout webhook (ADR-003 §C) and by the self-host install script
//!   (ADR-002 §I); the binary and the migration set are identical.

mod error;

pub use error::Error;
use error::{Result, env_var};

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
    /// Apply pending database migrations from `DATABASE_URL`
    /// (ADR-003 §A/§C).
    Migrate,
}

/// Parse the CLI args and dispatch the chosen subcommand.
pub async fn run() -> Result<()> {
    match Cli::parse().command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
        Command::EmitSpec => emit_spec(),
        Command::Migrate => migrate().await,
    }
}

fn emit_spec() -> Result<()> {
    let json = flight_academy_api::openapi().to_pretty_json()?;
    println!("{json}");
    Ok(())
}

async fn serve() -> Result<()> {
    init_tracing();

    // Secure-by-default per ADR-004 §F / ADR-016 §E: loopback-only for
    // native `cargo run`. Containers (Dockerfile / Helm / docker-compose)
    // override with `BIND_ADDR=0.0.0.0:8080` since the container's network
    // namespace is the security boundary there.
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "flight-academy listening");
    axum::serve(listener, flight_academy_api::app()).await?;
    Ok(())
}

async fn migrate() -> Result<()> {
    init_tracing();

    let database_url = env_var("DATABASE_URL")?;
    let db = flight_academy_db::Db::connect(&database_url).await?;
    db.migrate().await?;
    tracing::info!("migrations applied");
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();
}
