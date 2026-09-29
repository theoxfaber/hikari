#![warn(missing_docs)]
//! License keys for Hikari Pro.
//!
//! Model: open core (stills, MIT/Apache) + source-available Pro (motion, PDF).
//! Keys are `ed25519`-signed offline; verification needs no network.
//! Enforcement is the key check plus the commercial terms — the check is
//! trivially removable from source, exactly like Sidekiq's model, and that is
//! stated openly instead of pretending otherwise.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// What a key unlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Feature {
    /// Animated encoders (GIF now, WebP/APNG next).
    Animate,
    /// PDF backend (ships next after motion).
    Pdf,
}

/// Plan encoded in a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plan {
    /// Free tier: stills only.
    Free,
    /// Pro tier: stills + motion + PDF.
    Pro,
    /// Scale tier: Pro + redistribution rights.
    Scale,
}

impl Plan {
    /// True when this plan includes `feature`.
    #[must_use]
    pub const fn allows(self, feature: Feature) -> bool {
        match self {
            Self::Free => false,
            Self::Pro | Self::Scale => match feature {
                Feature::Animate | Feature::Pdf => true,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Payload {
    plan: Plan,
    /// Expiry as unix seconds (`0` = never).
    exp: u64,
    /// Key id for revocation lists.
    kid: String,
}

/// A verified license key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct License {
    plan: Plan,
    /// Expiry unix seconds (`0` = never expires).
    pub expires: u64,
    /// Key id.
    pub kid: String,
    /// Dev/test keys bypass nothing except expiry — they are rate-limited by
    /// honesty, not by code. Production keys come from the signing service.
    pub dev: bool,
}

impl License {
    /// Parse + verify `key` against `trusted_pubkey` (32 bytes).
    /// Format: `hk1.<base64url(payload-json)>.<base64url(signature)>`.
    pub fn verify(
        key: &str,
        trusted_pubkey: &[u8; 32],
        now_unix: u64,
    ) -> Result<Self, LicenseError> {
        let rest = key.strip_prefix("hk1.").ok_or(LicenseError::Format)?;
        let (payload_b64, sig_b64) = rest.split_once('.').ok_or(LicenseError::Format)?;
        let payload_bytes = b64decode(payload_b64)?;
        let sig_bytes = b64decode(sig_b64)?;
        if sig_bytes.len() != 64 {
            return Err(LicenseError::Format);
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&sig_arr);
        let pubkey = VerifyingKey::from_bytes(trusted_pubkey).map_err(|_| LicenseError::BadKey)?;
        pubkey
            .verify(&payload_bytes, &signature)
            .map_err(|_| LicenseError::BadSignature)?;
        let payload: Payload =
            serde_json::from_slice(&payload_bytes).map_err(|_| LicenseError::Format)?;
        if payload.exp != 0 && now_unix > payload.exp {
            return Err(LicenseError::Expired);
        }
        Ok(Self {
            plan: payload.plan,
            expires: payload.exp,
            kid: payload.kid,
            dev: false,
        })
    }

    /// Dev/test license. Always `Pro`, expires in 24h from `now_unix`.
    /// For tests and local evaluation — production requires a signed key.
    #[must_use]
    pub fn dev(now_unix: u64) -> Self {
        Self {
            plan: Plan::Pro,
            expires: now_unix + 86_400,
            kid: "dev".to_owned(),
            dev: true,
        }
    }

    /// True when `feature` is allowed and the key is unexpired at `now_unix`.
    #[must_use]
    pub fn allows_at(&self, feature: Feature, now_unix: u64) -> bool {
        if self.expires != 0 && now_unix > self.expires {
            return false;
        }
        self.plan.allows(feature)
    }

    /// Mint a signed key (signing-service side; kept here so tests stay honest).
    #[must_use]
    pub fn mint(plan: Plan, exp: u64, kid: &str, signing_key: &[u8; 32]) -> String {
        let payload = Payload {
            plan,
            exp,
            kid: kid.to_owned(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("payload serializes");
        let sk = SigningKey::from_bytes(signing_key);
        let sig = sk.sign(&payload_bytes);
        format!(
            "hk1.{}.{}",
            b64encode(&payload_bytes),
            b64encode(&sig.to_bytes())
        )
    }
}

/// License errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LicenseError {
    /// Malformed key string.
    #[error("malformed license key")]
    Format,
    /// Signature does not verify.
    #[error("bad license signature")]
    BadSignature,
    /// Key expired.
    #[error("license expired")]
    Expired,
    /// Feature not in plan.
    #[error("plan does not include this feature")]
    NotEntitled,
    /// Malformed public key.
    #[error("bad public key")]
    BadKey,
}

fn b64encode(bytes: &[u8]) -> String {
    base64::engine::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

fn b64decode(s: &str) -> Result<Vec<u8>, LicenseError> {
    base64::engine::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, s)
        .map_err(|_| LicenseError::Format)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic test keypair (NOT secret — tests only).
    const TEST_SK: [u8; 32] = [7u8; 32];

    fn test_pubkey() -> [u8; 32] {
        SigningKey::from_bytes(&TEST_SK).verifying_key().to_bytes()
    }

    #[test]
    fn roundtrip_verifies() {
        let pk = test_pubkey();
        let key = License::mint(Plan::Pro, 0, "test-1", &TEST_SK);
        let lic = License::verify(&key, &pk, 1_700_000_000).unwrap();
        assert!(lic.allows_at(Feature::Animate, 1_700_000_000));
        assert!(lic.allows_at(Feature::Pdf, 1_700_000_000));
        assert!(!lic.dev);
    }

    #[test]
    fn free_plan_denies_pro_features() {
        let pk = test_pubkey();
        let key = License::mint(Plan::Free, 0, "free-1", &TEST_SK);
        let lic = License::verify(&key, &pk, 1_700_000_000).unwrap();
        assert!(!lic.allows_at(Feature::Animate, 1_700_000_000));
    }

    #[test]
    fn tampered_payload_rejected() {
        let pk = test_pubkey();
        let key = License::mint(Plan::Pro, 0, "x", &TEST_SK);
        // Flip a payload char (keep base64 alphabet valid).
        let mut bad = key.into_bytes();
        let dot = bad.iter().position(|&b| b == b'.').unwrap();
        bad[dot + 2] = if bad[dot + 2] == b'A' { b'B' } else { b'A' };
        let bad = String::from_utf8(bad).unwrap();
        assert_eq!(
            License::verify(&bad, &pk, 1_700_000_000),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn expired_rejected() {
        let pk = test_pubkey();
        let key = License::mint(Plan::Pro, 1_000, "old", &TEST_SK);
        assert_eq!(
            License::verify(&key, &pk, 2_000),
            Err(LicenseError::Expired)
        );
    }
}
