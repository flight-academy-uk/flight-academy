//! The startup server-version check (`Db::verify_server_version`) accepts
//! the PostgreSQL server the test suite runs against. The comparison
//! itself, including the refusal of an older server, is unit-tested in
//! the crate.

use flight_academy_test_support::fresh_db;

#[tokio::test]
async fn test_server_meets_the_minimum_version() {
    let db = fresh_db().await;
    db.verify_server_version()
        .await
        .expect("the test container runs a supported PostgreSQL");
}
