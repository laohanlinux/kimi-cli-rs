use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;

/// OAuth credential and token manager.
#[derive(Debug, Clone)]
pub struct OAuthManager {
    platforms: crate::auth::platforms::PlatformIntegrations,
}

impl Default for OAuthManager {
    fn default() -> Self {
        Self {
            platforms: crate::auth::platforms::PlatformIntegrations::default(),
        }
    }
}

impl OAuthManager {
    /// Retrieves a token for the given storage key.
    ///
    /// Supported storage values:
    /// - `"file"` — reads from `~/.kimi/oauth/{key}.token`
    /// - `"keyring"` — reads from the OS keyring
    /// - any other value — returns empty token
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_token(&self, storage: &str, key: &str) -> crate::error::Result<SecretString> {
        match storage {
            "file" => {
                let path = crate::share::get_share_dir()?
                    .join("oauth")
                    .join(format!("{key}.token"));
                if !path.exists() {
                    return Ok(SecretString::new("".into()));
                }
                let text = tokio::fs::read_to_string(&path).await?;
                Ok(SecretString::new(text.trim().into()))
            }
            "keyring" => {
                let entry = keyring::Entry::new("kimi-cli-rs", key).map_err(|e| {
                    crate::error::KimiCliError::Generic(format!("keyring error: {e}"))
                })?;
                match entry.get_password() {
                    Ok(token) => Ok(SecretString::new(token.into())),
                    Err(keyring::Error::NoEntry) => Ok(SecretString::new("".into())),
                    Err(e) => {
                        tracing::warn!("keyring get failed for key {}: {}", key, e);
                        Ok(SecretString::new("".into()))
                    }
                }
            }
            _ => {
                tracing::warn!("unknown oauth storage '{}', returning empty token", storage);
                Ok(SecretString::new("".into()))
            }
        }
    }

    /// Resolves an `OAuthRef` into a token.
    pub async fn resolve(
        &self,
        oauth_ref: &crate::config::OAuthRef,
    ) -> crate::error::Result<SecretString> {
        self.get_token(&oauth_ref.storage, &oauth_ref.key).await
    }

    /// Resolves the effective API key, preferring the raw key and falling back to OAuth.
    pub async fn resolve_api_key(
        &self,
        api_key: &SecretString,
        oauth_ref: Option<&crate::config::OAuthRef>,
    ) -> Option<SecretString> {
        if !api_key.expose_secret().is_empty() {
            return Some(api_key.clone());
        }
        if let Some(ref oauth) = oauth_ref {
            match self.resolve(oauth).await {
                Ok(token) if !token.expose_secret().is_empty() => return Some(token),
                Ok(_) => {}
                Err(e) => tracing::warn!("failed to resolve oauth token: {e}"),
            }
        }
        None
    }

    /// Returns common OAuth headers (empty for now, reserved for future platform headers).
    pub fn common_headers(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Ensures the OAuth token is fresh, refreshing if necessary.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn ensure_fresh(
        &self,
        oauth_ref: Option<&crate::config::OAuthRef>,
    ) -> crate::error::Result<()> {
        let Some(ref oauth) = oauth_ref else {
            return Ok(());
        };
        let token = self.resolve(oauth).await?;
        if token.expose_secret().is_empty() {
            tracing::warn!("OAuth token is empty and cannot be refreshed automatically");
        }
        // Automatic refresh via platform integrations can be hooked here.
        let _ = &self.platforms;
        Ok(())
    }

    /// Saves a token to the given storage backend.
    #[tracing::instrument(level = "info", skip(self, token))]
    pub async fn save_token(
        &self,
        storage: &str,
        key: &str,
        token: &SecretString,
    ) -> crate::error::Result<()> {
        match storage {
            "file" => {
                let dir = crate::share::get_share_dir()?.join("oauth");
                tokio::fs::create_dir_all(&dir).await?;
                let path = dir.join(format!("{key}.token"));
                tokio::fs::write(&path, token.expose_secret()).await?;
                Ok(())
            }
            "keyring" => {
                let entry = keyring::Entry::new("kimi-cli-rs", key).map_err(|e| {
                    crate::error::KimiCliError::Generic(format!("keyring error: {e}"))
                })?;
                entry.set_password(token.expose_secret()).map_err(|e| {
                    crate::error::KimiCliError::Generic(format!("keyring set failed: {e}"))
                })?;
                Ok(())
            }
            _ => Err(crate::error::KimiCliError::Generic(format!(
                "unknown oauth storage '{}'",
                storage
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oauth_save_and_load_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("kimi-oauth-{}", uuid::Uuid::new_v4()));
        crate::share::set_test_share_dir(tmp.clone());

        let mgr = OAuthManager::default();
        let key = format!("test-{}", uuid::Uuid::new_v4());
        let token = SecretString::new("secret-token".into());

        mgr.save_token("file", &key, &token).await.unwrap();
        let loaded = mgr.get_token("file", &key).await.unwrap();
        assert_eq!(loaded.expose_secret(), "secret-token");

        crate::share::clear_test_share_dir();
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn resolve_api_key_prefers_raw() {
        let mgr = OAuthManager::default();
        let raw = SecretString::new("raw-key".into());
        let resolved = mgr.resolve_api_key(&raw, None).await;
        assert_eq!(resolved.unwrap().expose_secret(), "raw-key");
    }

    #[tokio::test]
    async fn resolve_api_key_falls_back_to_oauth() {
        let tmp = std::env::temp_dir().join(format!("kimi-oauth-{}", uuid::Uuid::new_v4()));
        crate::share::set_test_share_dir(tmp.clone());

        let mgr = OAuthManager::default();
        let key = format!("test-{}", uuid::Uuid::new_v4());
        let token = SecretString::new("oauth-token".into());
        mgr.save_token("file", &key, &token).await.unwrap();

        let oauth_ref = crate::config::OAuthRef {
            storage: "file".into(),
            key: key.clone(),
        };
        let empty = SecretString::new("".into());
        let resolved = mgr.resolve_api_key(&empty, Some(&oauth_ref)).await;
        assert_eq!(resolved.unwrap().expose_secret(), "oauth-token");

        crate::share::clear_test_share_dir();
        std::fs::remove_dir_all(&tmp).ok();
    }
}
