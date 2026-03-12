//! OIDC state management with caching for discovery and JWKS.
//!
//! This module provides the OidcState struct which manages OIDC discovery
//! document and JWKS caching for JWT token verification.

use std::sync::Arc;
use std::time::{Duration, Instant};

use backend_core::AppResult;
use tokio::sync::RwLock;

use crate::document::DiscoveryDocument;
use crate::http_client::HttpClient;

/// OIDC state for managing discovery document and JWKS caching.
#[derive(Clone)]
pub struct OidcState {
    /// Expected audience(s) for JWT validation
    pub(crate) audiences: Option<Vec<String>>,
    /// OIDC issuer URL
    issuer: String,
    /// Time-to-live for cached discovery document
    discovery_ttl: Duration,
    /// Time-to-live for cached JWKS
    jwks_ttl: Duration,
    /// HTTP client for fetching OIDC documents
    http: HttpClient,
    /// Internal state with cached data
    inner: Arc<RwLock<Inner>>,
}

/// Internal state holding cached OIDC data with timestamps.
#[derive(Clone)]
struct Inner {
    discovery: Option<(Arc<DiscoveryDocument>, Instant)>,
    jwks: Option<(Arc<jsonwebtoken::jwk::JwkSet>, Instant)>,
}

impl std::fmt::Debug for OidcState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("issuer", &"<String>")
            .field("discovery_ttl", &"<Duration>")
            .field("jwks_ttl", &"<Duration>")
            .field("http", &"<HttpClient>")
            .finish()
    }
}

impl OidcState {
    /// Creates a new OidcState instance.
    pub fn new(
        issuer: String,
        audiences: Option<Vec<String>>,
        discovery_ttl: Duration,
        jwks_ttl: Duration,
        http: HttpClient,
    ) -> Self {
        Self {
            audiences,
            issuer,
            discovery_ttl,
            jwks_ttl,
            http,
            inner: Arc::new(RwLock::new(Inner {
                discovery: None,
                jwks: None,
            })),
        }
    }

    /// Returns the OIDC discovery document, fetching and caching if necessary.
    /// Uses double-checked locking pattern for thread safety.
    #[tracing::instrument(skip(self))]
    pub async fn get_discovery(&self) -> AppResult<Arc<DiscoveryDocument>> {
        let now = Instant::now();
        let mut inner = self.inner.write().await;
        // Check again after acquiring write lock
        if let Some((doc, fetched)) = &inner.discovery
            && now.duration_since(*fetched) < self.discovery_ttl
        {
            return Ok(doc.clone());
        }

        let url = format!("{}/.well-known/openid-configuration", self.issuer);
        let doc: DiscoveryDocument = self.http.fetch_json(&url).await?;
        let doc = Arc::new(doc);
        inner.discovery = Some((doc.clone(), Instant::now()));
        Ok(doc)
    }

    /// Returns the JWKS (JSON Web Key Set) for token verification, fetching and caching if necessary.
    /// Automatically fetches discovery document if not cached or expired.
    #[tracing::instrument(skip(self))]
    pub async fn get_jwks(&self) -> AppResult<Arc<jsonwebtoken::jwk::JwkSet>> {
        let now = Instant::now();
        let mut inner = self.inner.write().await;
        // Check again after acquiring write lock
        if let Some((jwks, fetched)) = &inner.jwks
            && now.duration_since(*fetched) < self.jwks_ttl
        {
            return Ok(jwks.clone());
        }

        let doc = match &inner.discovery {
            Some((d, fetched)) if now.duration_since(*fetched) < self.discovery_ttl => d.clone(),
            _ => {
                drop(inner);
                let doc = self.get_discovery().await?;
                inner = self.inner.write().await;
                doc
            }
        };

        let jwks: jsonwebtoken::jwk::JwkSet = self.http.fetch_json(&doc.jwks_uri).await?;
        let jwks = Arc::new(jwks);
        inner.jwks = Some((jwks.clone(), Instant::now()));
        Ok(jwks)
    }
}
