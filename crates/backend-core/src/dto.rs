use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Display;
use utoipa::ToSchema;

const ACTIVE: &str = "active";
const REVOKED: &str = "revoked";

fn default_limits() -> Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    pub billing_identity: String,
    #[serde(default)]
    pub owners_admins: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateAccount {
    pub billing_identity: String,
    #[serde(default)]
    pub owners_admins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateAccount {
    pub billing_identity: Option<String>,
    pub owners_admins: Option<Vec<String>>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub allowed_models: Option<Vec<String>>,
    #[schema(value_type = Object)]
    pub default_limits: Option<Value>,
    pub billing_plan: Option<String>,
}
