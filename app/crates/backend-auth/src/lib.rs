//! Authentication and authorization library for the tokenization backend.
#![allow(clippy::result_large_err)]
//!
//! This crate provides JWT token validation, OIDC discovery integration,
//! Keycloak signature verification, and HTTP middleware for request authentication.
//! It supports three API surfaces: KC (Keycloak), BFF (Backend-for-Frontend), and Staff.

mod claims;
mod document;
mod http_client;
mod jwt_token;
mod middleware;
mod oidc_state;
mod signature_principal;

// Re-export all public types
pub use claims::*;
pub use document::*;
pub use http_client::*;
pub use jwt_token::*;
pub use middleware::*;
pub use oidc_state::*;
pub use signature_principal::*;
