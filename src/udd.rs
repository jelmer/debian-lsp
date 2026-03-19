//! Shared connection pool for the Ultimate Debian Database (UDD).

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const UDD_URL: &str = "postgres://udd-mirror:udd-mirror@udd-mirror.debian.net/udd";

/// A shared UDD connection pool that can be cloned cheaply.
pub type SharedPool = Arc<PgPool>;

/// Create a shared lazy connection pool to UDD.
pub fn shared_pool() -> SharedPool {
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy(UDD_URL)
        .expect("invalid UDD connection URL");
    Arc::new(pool)
}
