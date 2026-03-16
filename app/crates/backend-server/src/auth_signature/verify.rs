use backend_core::{Error, Result};
use base64::Engine;
use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::sign::Verifier;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Decodes base64url standard into raw bytes.
fn decode_base64_url(encoded: &str) -> Result<Vec<u8>> {
    let encoded = encoded.trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| {
            tracing::error!(
                error = %e,
                signature_b64url = %encoded,
                "Failed to decode base64url signature"
            );
            Error::unauthorized("Invalid base64url encoding")
        })
}

#[derive(Deserialize, Debug)]
struct Jwk {
    kty: String,

    // For RSA
    n: Option<String>,
    e: Option<String>,

    // For EC
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

/// Parses the JWK and verifies the signature over the canonical payload.
///
/// Supported key types:
/// - EC (P-256)
/// - RSA
pub fn verify_signature(
    public_key_json: &str,
    canonical_payload: &str,
    signature_base64url: &str,
) -> Result<()> {
    let jwk: Jwk = serde_json::from_str(public_key_json)
        .map_err(|_| Error::unauthorized("Invalid JWK JSON format"))?;

    let signature_bytes = decode_base64_url(signature_base64url)?;

    match jwk.kty.as_str() {
        "RSA" => verify_rsa(&jwk, canonical_payload, &signature_bytes),
        "EC" => verify_ec(&jwk, canonical_payload, &signature_bytes),
        unsupported => Err(Error::unauthorized(&format!(
            "Unsupported key type: {}",
            unsupported
        ))),
    }
}

fn verify_rsa(jwk: &Jwk, payload: &str, signature: &[u8]) -> Result<()> {
    let n_str = jwk
        .n
        .as_ref()
        .ok_or_else(|| Error::unauthorized("Missing n for RSA"))?;
    let e_str = jwk
        .e
        .as_ref()
        .ok_or_else(|| Error::unauthorized("Missing e for RSA"))?;

    let n = decode_base64_url(n_str)?;
    let e = decode_base64_url(e_str)?;

    let n_bn = BigNum::from_slice(&n).map_err(|_| Error::unauthorized("Invalid RSA modulus"))?;
    let e_bn = BigNum::from_slice(&e).map_err(|_| Error::unauthorized("Invalid RSA exponent"))?;

    let rsa = Rsa::from_public_components(n_bn, e_bn)
        .map_err(|_| Error::unauthorized("Failed to construct RSA key"))?;

    let pkey = PKey::from_rsa(rsa).map_err(|_| Error::unauthorized("Failed to construct PKey"))?;

    let mut verifier = Verifier::new(MessageDigest::sha256(), &pkey)
        .map_err(|_| Error::unauthorized("Failed to init RSA verifier"))?;

    verifier
        .update(payload.as_bytes())
        .map_err(|_| Error::unauthorized("Failed to hash RSA payload"))?;

    let is_valid = verifier
        .verify(signature)
        .map_err(|_| Error::unauthorized("RSA signature verification failed"))?;

    if !is_valid {
        return Err(Error::unauthorized("Invalid signature"));
    }

    Ok(())
}

fn verify_ec(jwk: &Jwk, payload: &str, signature: &[u8]) -> Result<()> {
    let crv = jwk.crv.as_deref().unwrap_or_default();
    if crv != "P-256" {
        return Err(Error::unauthorized(&format!(
            "Unsupported EC curve: {}",
            crv
        )));
    }

    let x_str = jwk
        .x
        .as_ref()
        .ok_or_else(|| Error::unauthorized("Missing x for EC"))?;
    let y_str = jwk
        .y
        .as_ref()
        .ok_or_else(|| Error::unauthorized("Missing y for EC"))?;

    let x = decode_base64_url(x_str)?;
    let y = decode_base64_url(y_str)?;

    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)
        .map_err(|_| Error::unauthorized("Failed to load P-256 curve"))?;

    let x_bn =
        BigNum::from_slice(&x).map_err(|_| Error::unauthorized("Invalid EC x coordinate"))?;
    let y_bn =
        BigNum::from_slice(&y).map_err(|_| Error::unauthorized("Invalid EC y coordinate"))?;

    let ec_key = EcKey::from_public_key_affine_coordinates(&group, &x_bn, &y_bn)
        .map_err(|_| Error::unauthorized("Failed to construct EC key"))?;

    // WebCrypto / ES256 produces raw r || s
    // For P-256, r and s are each 32 bytes, total 64 bytes.
    if signature.len() != 64 {
        return Err(Error::unauthorized("Invalid EC signature length"));
    }

    let r_bn = BigNum::from_slice(&signature[0..32])
        .map_err(|_| Error::unauthorized("Invalid EC r component"))?;
    let s_bn = BigNum::from_slice(&signature[32..64])
        .map_err(|_| Error::unauthorized("Invalid EC s component"))?;

    let sig = EcdsaSig::from_private_components(r_bn, s_bn)
        .map_err(|_| Error::unauthorized("Failed to parse ECDSA signature"))?;

    let digest = Sha256::digest(payload.as_bytes());

    let is_valid = sig
        .verify(&digest, &ec_key)
        .map_err(|_| Error::unauthorized("EC signature verification failed"))?;

    if !is_valid {
        return Err(Error::unauthorized("Invalid signature"));
    }

    Ok(())
}
