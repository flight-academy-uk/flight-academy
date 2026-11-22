// `sqlx::migrate!` embeds the migration files at compile time. Without this,
// Cargo does not rebuild the crate when a migration is added or removed.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
