//! API key resolution: env first, then OS keyring (fail closed, no hang).

use crate::error::{AiError, Result};

/// Frozen env var name for headless / CI API key.
pub const AI_API_KEY_ENV: &str = "PST_DEDUPE_AI_API_KEY";

/// Keyring service name (Desk).
pub const KEYRING_SERVICE: &str = "dedupe-desk";

/// Keyring user / account for the AI API key (not matter-scoped in P0).
pub const KEYRING_USER: &str = "ai_api_key";

/// Pure env-value preference: non-empty trimmed env wins; empty/whitespace falls through.
///
/// Used by [`resolve_api_key`] and unit-tested without mutating process env
/// (`forbid(unsafe_code)` forbids `set_var` on modern Rust).
pub(crate) fn prefer_env_key(env_value: Option<&str>) -> Option<String> {
    env_value
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Resolve API key: **`PST_DEDUPE_AI_API_KEY`** env first, then OS keyring.
///
/// - Env wins when set (including empty → treated as missing, fall through).
/// - Keyring failures become [`AiError::ApiKeyError`] or missing — never hang.
/// - Returns [`AiError::ApiKeyMissing`] when neither source provides a non-empty key.
pub fn resolve_api_key() -> Result<String> {
    let env_raw = std::env::var(AI_API_KEY_ENV).ok();
    if let Some(k) = prefer_env_key(env_raw.as_deref()) {
        return Ok(k);
    }
    match read_keyring() {
        Ok(Some(k)) => Ok(k),
        Ok(None) => Err(AiError::ApiKeyMissing),
        Err(e) => Err(e),
    }
}

/// Optional resolve: `Ok(None)` when missing (caller decides if key is required).
pub fn resolve_api_key_optional() -> Result<Option<String>> {
    match resolve_api_key() {
        Ok(k) => Ok(Some(k)),
        Err(AiError::ApiKeyMissing) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Store API key in OS keyring (Desk path). Overwrites existing.
pub fn store_api_key(key: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AiError::ApiKeyError(format!("keyring entry create failed: {e}")))?;
    entry
        .set_password(key)
        .map_err(|e| AiError::ApiKeyError(format!("keyring set failed: {e}")))?;
    Ok(())
}

/// Delete key from keyring if present (ignore missing).
pub fn delete_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AiError::ApiKeyError(format!("keyring entry create failed: {e}")))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AiError::ApiKeyError(format!("keyring delete failed: {e}"))),
    }
}

fn read_keyring() -> Result<Option<String>> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => {
            return Err(AiError::ApiKeyError(format!("keyring unavailable: {e}")));
        }
    };
    match entry.get_password() {
        Ok(p) => {
            let t = p.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t.to_string()))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AiError::ApiKeyError(format!(
            "keyring read failed (treat as missing if headless): {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_path_without_keyring() {
        // Pure path: non-empty env value is preferred over keyring (no process env mutation).
        assert_eq!(
            prefer_env_key(Some("test-key-from-env-xyz")).as_deref(),
            Some("test-key-from-env-xyz")
        );
        assert_eq!(
            prefer_env_key(Some("  spaced-key  ")).as_deref(),
            Some("spaced-key")
        );
    }

    #[test]
    fn empty_env_falls_through() {
        assert!(prefer_env_key(Some("   ")).is_none());
        assert!(prefer_env_key(Some("")).is_none());
        assert!(prefer_env_key(None).is_none());
        // Full optional resolve must not hang / panic when env is unset or whitespace-only
        // (may return keyring value or None depending on host).
        let _ = resolve_api_key_optional();
    }
}
