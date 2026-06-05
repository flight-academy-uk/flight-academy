//! Thin entrypoint per ADR-005 §D — see [`cli`] for the CLI surface.

mod cli;

#[tokio::main]
async fn main() -> Result<(), cli::Error> {
    cli::run().await
}
