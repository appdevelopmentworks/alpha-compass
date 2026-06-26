//! Credential storage via the OS keychain.
//!
//! API tokens are NEVER stored in the databases or in plaintext. They live in
//! the OS keychain (Windows Credential Manager here) and are handed to the
//! sidecar as environment variables only at spawn time.

use crate::store::models::CredentialStatus;

const SERVICE: &str = "alpha-compass";

/// (credential key, sidecar environment variable). J-Quants v2 uses a single
/// API key via `x-api-key` (the v1 token/refresh flow was retired).
pub const CRED_SOURCES: &[(&str, &str)] = &[
    ("fred", "FRED_API_KEY"),
    ("jquants", "JQUANTS_API_KEY"),
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("edinet", "EDINET_API_KEY"),
];

/// Store (or, with an empty token, delete) a credential.
pub fn set_credential(source: &str, token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, source).map_err(|e| format!("keychain: {e}"))?;
    if token.is_empty() {
        let _ = entry.delete_credential();
        Ok(())
    } else {
        entry
            .set_password(token)
            .map_err(|e| format!("keychain set: {e}"))
    }
}

fn get_credential(source: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, source)
        .ok()?
        .get_password()
        .ok()
}

/// Which credentials are configured (values never exposed).
pub fn status() -> Vec<CredentialStatus> {
    CRED_SOURCES
        .iter()
        .map(|(s, _)| CredentialStatus {
            source: (*s).to_string(),
            configured: get_credential(s).is_some(),
        })
        .collect()
}

/// Environment variables to inject into the sidecar for configured creds.
pub fn sidecar_env() -> Vec<(String, String)> {
    CRED_SOURCES
        .iter()
        .filter_map(|(s, env)| get_credential(s).map(|v| ((*env).to_string(), v)))
        .collect()
}
