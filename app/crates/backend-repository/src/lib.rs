// Ensure OpenSSL native libs are linked when libpq is built from pq-src on Linux.
#[cfg(target_os = "linux")]
extern crate openssl_sys as _;

mod pg;
mod traits;

pub use pg::*;
pub use traits::*;
