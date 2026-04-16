use secrecy::{ExposeSecret, SecretString};

/// OAuth credential and token manager.
#[derive(Debug, Clone, Default)]
pub struct OAuthManager;

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
                let path = crate::share::get_share_dir()?.join("oauth").join(format!("{key}.token"));
                if !path.exists() {
                    return Ok(SecretString::new("".into()));
                }
                let text = tokio::fs::read_to_string(&path).await?;
                Ok(SecretString::new(text.trim().into()))
            }
            "keyring" => {
                let entry = keyring::Entry::new("kimi-cli-rs", key)
                    .map_err(|e| crate::error::KimiCliError::Generic(format!("keyring error: {e}")))?;
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
    pub async fn resolve(&self, oauth_ref: &crate::config::OAuthRef) -> crate::error::Result<SecretString> {
        self.get_token(&oauth_ref.storage, &oauth_ref.key).await
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
                let entry = keyring::Entry::new("kimi-cli-rs", key)
                    .map_err(|e| crate::error::KimiCliError::Generic(format!("keyring error: {e}")))?;
                entry
                    .set_password(token.expose_secret())
                    .map_err(|e| crate::error::KimiCliError::Generic(format!("keyring set failed: {e}")))?;
                Ok(())
            }
            _ => Err(crate::error::KimiCliError::Generic(
                format!("unknown oauth storage '{}'", storage)
            )),
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
}
