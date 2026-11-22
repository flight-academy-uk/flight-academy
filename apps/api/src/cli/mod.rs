//! CLI surface for the `flight-academy` binary: subcommand parsing,
//! configuration and dispatch. `main.rs` only calls [`run`].
//!
//! Every setting is read once at startup, from a flag or its environment
//! variable. A required setting that is missing (`DATABASE_URL`) stops the
//! process before anything else happens.
//!
//! Subcommands:
//!
//! * `serve` (default): bind and serve the HTTP API.
//! * `emit-spec`: write the OpenAPI document as pretty JSON to stdout
//!   (ADR-006 §A; format per ADR-018).
//! * `migrate`: apply pending database migrations (ADR-003 §C).

mod error;

pub use error::Error;
use error::Result;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use clap::builder::BoolishValueParser;
use clap::{Args, Parser, Subcommand};
use flight_academy_auth::{ActorClass, Role, Subject, SubjectAttributes};
use flight_academy_db::PoolSettings;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "flight-academy", about = "Flight Academy API", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the HTTP API (default if no subcommand given).
    Serve(ServeArgs),
    /// Emit the OpenAPI document as pretty JSON to stdout.
    EmitSpec,
    /// Apply pending database migrations.
    Migrate(DatabaseArgs),
}

#[derive(Args)]
struct DatabaseArgs {
    /// PostgreSQL connection URL.
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    database_url: String,
}

#[derive(Args)]
struct ServeArgs {
    #[command(flatten)]
    database: DatabaseArgs,

    /// Address to listen on. Loopback by default; containers set 0.0.0.0.
    #[arg(long, env = "BIND_ADDR", default_value = "127.0.0.1:8080")]
    bind_addr: SocketAddr,

    /// Whole-request deadline in seconds, including the request body.
    #[arg(long, env = "FA_REQUEST_TIMEOUT_SECS", default_value_t = 30)]
    request_timeout_secs: u64,

    /// Largest request body in bytes.
    #[arg(long, env = "FA_BODY_LIMIT_BYTES", default_value_t = 256 * 1024)]
    body_limit_bytes: usize,

    /// Seconds to wait for in-flight requests after SIGTERM before exiting.
    #[arg(long, env = "FA_SHUTDOWN_GRACE_SECS", default_value_t = 25)]
    shutdown_grace_secs: u64,

    /// Maximum open database connections.
    #[arg(long, env = "FA_DB_MAX_CONNECTIONS", default_value_t = 10)]
    db_max_connections: u32,

    /// Seconds to wait for a free connection before failing the request.
    #[arg(long, env = "FA_DB_ACQUIRE_TIMEOUT_SECS", default_value_t = 5)]
    db_acquire_timeout_secs: u64,

    /// Per-statement limit in seconds, applied as `statement_timeout`.
    #[arg(long, env = "FA_DB_STATEMENT_TIMEOUT_SECS", default_value_t = 15)]
    db_statement_timeout_secs: u64,

    #[command(flatten)]
    dev_auth: DevAuthArgs,
}

/// Development-only identity, removed when real authentication lands.
#[derive(Args)]
struct DevAuthArgs {
    /// Inject a fixed subject on tenant routes. Never enable in production.
    #[arg(
        long = "dev-auth",
        env = "FA_DEV_AUTH",
        value_parser = BoolishValueParser::new(),
        default_value = "false",
        action = clap::ArgAction::Set
    )]
    enabled: bool,

    #[arg(long = "dev-user-id", env = "FA_DEV_USER_ID")]
    user_id: Option<Uuid>,

    #[arg(long = "dev-tenant-id", env = "FA_DEV_TENANT_ID")]
    tenant_id: Option<Uuid>,

    /// Comma-separated roles, e.g. `tenant-admin`. Unknown names are ignored.
    #[arg(long = "dev-roles", env = "FA_DEV_ROLES", default_value = "")]
    roles: String,
}

impl DevAuthArgs {
    fn subject(&self) -> Result<Option<Subject>> {
        if !self.enabled {
            return Ok(None);
        }
        let (Some(user_id), Some(tenant_id)) = (self.user_id, self.tenant_id) else {
            return Err(Error::Config {
                reason: "FA_DEV_AUTH is on but FA_DEV_USER_ID or FA_DEV_TENANT_ID is missing",
            });
        };
        let roles: BTreeSet<Role> = self
            .roles
            .split(',')
            .filter_map(|s| Role::from_str_known(s.trim()))
            .collect();
        Ok(Some(Subject {
            user_id,
            actor_class: ActorClass::Member,
            tenant_id: Some(tenant_id),
            roles,
            attributes: SubjectAttributes,
            elevation: None,
        }))
    }
}

/// Parse the CLI args and dispatch the chosen subcommand.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Serve(args)) => serve(args).await,
        None => serve(ServeArgs::from_env()).await,
        Some(Command::EmitSpec) => emit_spec(),
        Some(Command::Migrate(args)) => migrate(args).await,
    }
}

impl ServeArgs {
    /// `serve` is the default subcommand; parse its settings from the
    /// environment alone when no subcommand is given. A missing required
    /// setting prints clap's error and exits, as it does for `serve`.
    fn from_env() -> Self {
        #[derive(Parser)]
        #[command(name = "flight-academy")]
        struct Only {
            #[command(flatten)]
            serve: ServeArgs,
        }
        Only::parse_from(["flight-academy"]).serve
    }
}

fn emit_spec() -> Result<()> {
    let json = flight_academy_api::openapi().to_pretty_json()?;
    println!("{json}");
    Ok(())
}

async fn serve(args: ServeArgs) -> Result<()> {
    init_tracing();

    let pool = PoolSettings {
        max_connections: args.db_max_connections,
        acquire_timeout: Duration::from_secs(args.db_acquire_timeout_secs),
        idle_timeout: Duration::from_secs(300),
        max_lifetime: Duration::from_secs(1800),
        statement_timeout: Some(Duration::from_secs(args.db_statement_timeout_secs)),
    };
    let app_config = flight_academy_api::AppConfig {
        request_timeout: Duration::from_secs(args.request_timeout_secs),
        body_limit: args.body_limit_bytes,
        dev_subject: args.dev_auth.subject()?,
    };
    if app_config.dev_subject.is_some() {
        tracing::warn!("development authentication is enabled; never use this in production");
    }

    let db = flight_academy_db::Db::connect(&args.database.database_url, &pool).await?;
    // Refuse to start with a pool role that cannot drive the audit chain
    // writer; see `flight_academy_db::audit::verify_pool_role`.
    db.verify_audit_pool_role().await?;

    let listener = tokio::net::TcpListener::bind(args.bind_addr).await?;
    tracing::info!(addr = %args.bind_addr, "flight-academy listening");
    let app = flight_academy_api::app(&app_config).layer(axum::Extension(db));

    let (stopping_tx, stopping_rx) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        shutdown_signal().await;
        tracing::info!("shutdown signal received; draining in-flight requests");
        let _ = stopping_tx.send(());
    });
    let grace = Duration::from_secs(args.shutdown_grace_secs);
    tokio::select! {
        result = server => result?,
        () = async {
            if stopping_rx.await.is_ok() {
                tokio::time::sleep(grace).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            tracing::warn!(grace_secs = args.shutdown_grace_secs, "grace period elapsed; exiting with requests still in flight");
        }
    }
    Ok(())
}

async fn migrate(args: DatabaseArgs) -> Result<()> {
    init_tracing();

    // No statement timeout here: a migration may legitimately run long.
    let pool = PoolSettings {
        max_connections: 1,
        acquire_timeout: Duration::from_secs(30),
        idle_timeout: Duration::from_secs(300),
        max_lifetime: Duration::from_secs(1800),
        statement_timeout: None,
    };
    let db = flight_academy_db::Db::connect(&args.database_url, &pool).await?;
    db.migrate().await?;
    tracing::info!("migrations applied");
    Ok(())
}

/// Resolves on SIGTERM (what Kubernetes sends) or Ctrl-C.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            tracing::warn!("could not install the Ctrl-C handler; relying on SIGTERM");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => {
                tracing::warn!("could not install the SIGTERM handler; relying on Ctrl-C");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(enabled: bool, user: Option<Uuid>, tenant: Option<Uuid>, roles: &str) -> DevAuthArgs {
        DevAuthArgs {
            enabled,
            user_id: user,
            tenant_id: tenant,
            roles: roles.to_owned(),
        }
    }

    #[test]
    fn dev_auth_off_yields_no_subject() {
        assert!(dev(false, None, None, "").subject().unwrap().is_none());
    }

    #[test]
    fn dev_auth_on_without_ids_refuses_to_start() {
        let err = dev(true, Some(Uuid::nil()), None, "")
            .subject()
            .unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }

    #[test]
    fn dev_auth_on_builds_subject_once_and_ignores_unknown_roles() {
        let (user, tenant) = (Uuid::new_v4(), Uuid::new_v4());
        let subject = dev(true, Some(user), Some(tenant), "tenant-admin, no-such-role")
            .subject()
            .unwrap()
            .unwrap();
        assert_eq!(subject.user_id, user);
        assert_eq!(subject.tenant_id, Some(tenant));
        assert_eq!(subject.roles.len(), 1);
    }

    #[test]
    fn missing_database_url_is_a_parse_error() {
        // SAFETY: test-only; this test is the only one in the binary that
        // touches DATABASE_URL.
        unsafe { std::env::remove_var("DATABASE_URL") };
        assert!(Cli::try_parse_from(["flight-academy", "serve"]).is_err());
        assert!(Cli::try_parse_from(["flight-academy", "migrate"]).is_err());
        assert!(Cli::try_parse_from(["flight-academy", "emit-spec"]).is_ok());
    }
}
