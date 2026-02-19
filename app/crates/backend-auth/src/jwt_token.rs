use crate::claims::Claims;
use crate::oidc_state::OidcState;
use axum::extract::{FromRequest, Request, State};
use backend_core::AppResult;
use http::HeaderValue;
use tracing::{debug, error, info};

pub struct JwtToken {
    pub claims: Claims,
}

impl JwtToken {
    pub fn new(claims: Claims) -> JwtToken {
        JwtToken { claims }
    }
}

impl JwtToken {
    async fn from_request(
        header_value: Option<&HeaderValue>,
        oidc_state: OidcState,
    ) -> AppResult<JwtToken> {
        debug!("JWT extraction from Authorization header started");
        let token = header_value.and_then(|auth| auth.strip_prefix("Bearer "));

        if let Some(token) = token {
            debug!("Bearer token present; validating");
            match state.get_jwks().await {
                Ok(jwks) => match validate_token(token, jwks.as_ref(), &state.audiences).await {
                    Ok(claims) => {
                        info!(
                            "JWT validated for subject={} audiences={:?}",
                            claims.sub, state.audiences
                        );
                        Outcome::Success(JwtToken::new(claims))
                    }
                    Err(e) => {
                        error!("Could not get claims {}", e);
                        Outcome::Error((Status::Unauthorized, ()))
                    }
                },
                Err(e) => {
                    error!("Could not get JWKS {}", e);
                    Outcome::Error((Status::Unauthorized, ()))
                }
            }
        } else {
            info!("No Authorization bearer token found");
            Outcome::Error((Status::Unauthorized, ()))
        }
    }
}
