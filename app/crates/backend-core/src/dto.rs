//! Data Transfer Objects (DTOs) for account and project management.
//!
//! These types define the structure of data exchanged with external clients
//! and are used for API request/response serialization.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// Default value for project limits (empty JSON object).
fn default_limits() -> Value {
    serde_json::json!({})
}

/// Represents an account in the system.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    pub billing_identity: String,
    #[serde(default)]
    pub owners_admins: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request payload for creating a new account.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateAccount {
    pub billing_identity: String,
    #[serde(default)]
    pub owners_admins: Vec<String>,
}

/// Request payload for updating an existing account.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateAccount {
    pub billing_identity: Option<String>,
    pub owners_admins: Option<Vec<String>>,
}

/// Represents a project belonging to an account.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Project {
    pub id: String,
    pub account_id: String,
    pub name: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default = "default_limits")]
    #[schema(value_type = Object)]
    pub default_limits: Value,
    pub billing_plan: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request payload for creating a new project.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProject {
    pub name: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default = "default_limits")]
    #[schema(value_type = Object)]
    pub default_limits: Value,
    pub billing_plan: String,
}

/// Request payload for updating an existing project.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub allowed_models: Option<Vec<String>>,
    #[schema(value_type = Object)]
    pub default_limits: Option<Value>,
    pub billing_plan: Option<String>,
}
