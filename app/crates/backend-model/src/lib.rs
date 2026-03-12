//! Database models and schema definitions for the tokenization backend.
//!
//! This crate contains:
//! - Diesel table schema definitions
//! - Database row types (Queryable, Selectable, Insertable)
//! - DTO models for API surfaces (BFF, KC)
//!
//! All IDs use the prefixed CUID format from backend-id (usr_*, dvc_*, etc.).

pub mod bff;
pub mod db;
pub mod kc;

/// Diesel table schema - auto-generated, do not edit manually.
pub mod schema;

// Re-export commonly used crates
pub use chrono;
pub use serde_json;
