//! Shared connection pool for the Ultimate Debian Database (UDD).

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const UDD_URL: &str = "postgres://udd-mirror:udd-mirror@udd-mirror.debian.net/udd";

/// Create a lazy connection pool to UDD with a limited number of connections.
pub fn connect_lazy() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy(UDD_URL)
        .expect("invalid UDD connection URL")
}
