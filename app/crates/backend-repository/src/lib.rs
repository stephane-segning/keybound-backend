//! Repository layer for database operations using Diesel-async.
//!
//! This crate provides:
//! - Repository traits defining database operations
//! - PostgreSQL implementations using diesel-async
//! - Connection pooling via deadpool
//!
//! All errors are mapped to backend_core::Error for consistent handling.

// Ensure OpenSSL native libs are linked when libpq is built from pq-src on Linux.
#[cfg(target_os = "linux")]
extern crate openssl_sys as _;

mod pg;
mod traits;

pub use pg::*;
pub use traits::*;
