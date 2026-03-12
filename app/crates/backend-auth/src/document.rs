//! OIDC discovery document structures.
//!
//! Represents the OpenID Connect discovery document that contains
//! endpoints and metadata for the OAuth2/OIDC provider.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OIDC discovery document containing provider metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveryDocument {
    /// Issuer URL for the OIDC provider
    pub issuer: String,
    /// URL for the authorization endpoint
    pub authorization_endpoint: String,
    /// URL for the token endpoint
    pub token_endpoint: String,
    /// URL for fetching the JSON Web Key Set
    pub jwks_uri: String,

    /// Additional fields from the discovery document
    #[serde(flatten)]
    pub extra: Value,
}
