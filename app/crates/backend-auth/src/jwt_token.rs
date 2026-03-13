//! JWT token handling and verification.
//!
//! Provides JWT token parsing, validation against JWKS, and claim extraction.

use crate::claims::Claims;
use crate::oidc_state::OidcState;
use backend_core::{Error, Result};
use jsonwebtoken::{DecodingKey, Validation, decode};
use tracing::instrument;

/// Wrapper around JWT claims with verification capabilities.
#[derive(Debug, Clone)]
pub struct JwtToken {
    pub claims: Claims,
}

impl JwtToken {
    /// Creates a new JwtToken from parsed claims (typically after successful verification).
    pub fn new(claims: Claims) -> JwtToken {
        JwtToken { claims }
    }

    /// Returns the user ID from the token's subject claim.
    pub fn user_id(&self) -> &str {
        if self.claims.sub.contains(&":".to_string()) {
            return &self
                .claims
                .sub
                .split(":")
                .collect::<Vec<&str>>()
                .last()
                .unwrap();
        }

        &self.claims.sub
    }

    /// Verifies a JWT token against the OIDC state's JWKS.
    ///
    /// Steps:
    /// 1. Fetch JWKS from OIDC state (with caching)
    /// 2. Extract the key ID (kid) from the token header
    /// 3. Find the matching JWK in the JWKS
    /// 4. Create a decoding key from the JWK
    /// 5. Validate the token signature and claims
    #[instrument(skip(oidc_state))]
    pub async fn verify(token: &str, oidc_state: &OidcState) -> Result<Self> {
        let jwks = oidc_state.get_jwks().await?;

        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| Error::unauthorized(format!("Invalid token header: {e}")))?;

        let kid = header
            .kid
            .ok_or_else(|| Error::unauthorized("Missing kid in token header"))?;

        let jwk = jwks
            .find(&kid)
            .ok_or_else(|| Error::unauthorized(format!("Key ID {kid} not found in JWKS")))?;

        let decoding_key = DecodingKey::from_jwk(jwk)
            .map_err(|e| Error::unauthorized(format!("Invalid JWK: {e}")))?;

        let mut validation = Validation::new(header.alg);
        if let Some(audiences) = &oidc_state.audiences {
            validation.set_audience(audiences);
        } else {
            validation.validate_aud = false;
        }

        let token_data = decode::<Claims>(token, &decoding_key, &validation)
            .map_err(|e| Error::unauthorized(format!("Token validation failed: {e}")))?;

        Ok(JwtToken::new(token_data.claims))
    }
}
