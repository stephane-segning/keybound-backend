//! Core library for the tokenization backend system.
//!
//! This crate provides foundational types and abstractions used across all other crates
//! in the workspace, including:
//! - Configuration management with environment variable support
//! - Error handling with rich metadata and HTTP status mapping
//! - Data transfer objects (DTOs) for account and project management
//! - Re-exports of commonly used dependencies

mod config;
mod dto;
mod error;

// Re-export DTOs for account and project management
pub use crate::dto::{
    Account, CreateAccount, CreateProject, Project, UpdateAccount, UpdateProject,
};

// Re-export error handling types
pub use crate::error::{AppResult, Error, ErrorMeta, ErrorPayload, Result};

// Re-export configuration types
pub use config::*;

// Re-export commonly used external crates
pub use anyhow;
pub use async_trait::async_trait;
pub use cuid;
