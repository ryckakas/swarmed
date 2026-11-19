//! API keys: where they are kept, and which ones Swarmed knows how to hand to an agent.
//!
//! A key is written once and afterwards only ever read to be placed in an agent's
//! environment. It is never returned to a front end, logged, or stored in Swarmed's own files.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::{Core, lock};

/// The providers a key can be stored for: the environment variable agents read it from, and
/// the provider's name.
const PROVIDERS: &[(&str, &str)] = &[
    ("OPENROUTER_API_KEY", "OpenRouter"),
    ("OPENAI_API_KEY", "OpenAI"),
    ("ANTHROPIC_API_KEY", "Anthropic API"),
    ("GEMINI_API_KEY", "Google Gemini"),
];

/// Somewhere secrets are kept by name.
pub trait SecretStore: fmt::Debug + Send + Sync {
    /// The secret stored under `name`, if there is one.
    fn get(&self, name: &str) -> Result<Option<String>>;
    /// Stores `value` under `name`, replacing what was there.
    fn set(&self, name: &str, value: &str) -> Result<()>;
    /// Removes the secret stored under `name`. Not an error if there is none.
    fn delete(&self, name: &str) -> Result<()>;
}

/// Secrets held in memory until the process ends: for tests, and for platforms without a
/// keychain Swarmed knows.
#[derive(Default)]
pub struct MemorySecrets(Mutex<BTreeMap<String, String>>);

impl fmt::Debug for MemorySecrets {
    /// Never prints the secrets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MemorySecrets(<redacted>)")
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(lock(&self.0).get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        lock(&self.0).insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        lock(&self.0).remove(name);
        Ok(())
    }
}

/// The user's login keychain on macOS.
#[cfg(target_os = "macos")]
#[derive(Debug, Default, Clone, Copy)]
pub struct Keychain;

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::base::Error as KeychainError;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    use super::{Error, Keychain, Result, SecretStore};

    /// The name every entry is filed under in Keychain Access.
    const SERVICE: &str = "Swarmed";
    /// `errSecItemNotFound`: there is no such entry.
    const NOT_FOUND: i32 = -25300;

    fn failed(error: KeychainError) -> Error {
        Error::Secrets(error.to_string())
    }

    /// Turns "no such entry" into `None` and passes every other failure on.
    fn found<T>(result: std::result::Result<T, KeychainError>) -> Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code() == NOT_FOUND => Ok(None),
            Err(error) => Err(failed(error)),
        }
    }

    impl SecretStore for Keychain {
        fn get(&self, name: &str) -> Result<Option<String>> {
            let bytes = found(get_generic_password(SERVICE, name))?;
            Ok(bytes.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
        }

        fn set(&self, name: &str, value: &str) -> Result<()> {
            set_generic_password(SERVICE, name, value.as_bytes()).map_err(failed)
        }

        fn delete(&self, name: &str) -> Result<()> {
            found(delete_generic_password(SERVICE, name)).map(drop)
        }
    }
}

/// Where API keys are kept on this platform.
#[must_use]
pub fn platform_store() -> Box<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    return Box::new(Keychain);
    #[cfg(not(target_os = "macos"))]
    return Box::new(MemorySecrets::default());
}

/// One provider a key can be stored for, and whether one is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiKeyInfo {
    /// The environment variable agents read the key from; also the key's name in the store.
    pub name: String,
    /// The provider's name.
    pub provider: String,
    /// Whether a key is stored.
    pub stored: bool,
}

/// Environment variables holding secrets, for one agent process.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretEnv(pub Vec<(String, String)>);

impl fmt::Debug for SecretEnv {
    /// Prints the names and never the values.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self.0.iter().map(|(name, _)| name);
        f.debug_list().entries(names).finish()
    }
}

fn known(name: &str) -> Result<&'static str> {
    let provider = PROVIDERS.iter().find(|(known, _)| *known == name);
    provider
        .map(|(known, _)| *known)
        .ok_or_else(|| Error::UnknownApiKey(name.to_string()))
}

impl Core {
    /// The providers a key can be stored for, and which have one. Never the keys themselves.
    pub fn api_keys(&self) -> Result<Vec<ApiKeyInfo>> {
        let info = |(name, provider): &(&str, &str)| {
            Ok(ApiKeyInfo {
                name: (*name).to_string(),
                provider: (*provider).to_string(),
                stored: self.secrets.get(name)?.is_some(),
            })
        };
        PROVIDERS.iter().map(info).collect()
    }

    /// Stores a provider's key. Sessions started from now on get it.
    pub fn set_api_key(&self, name: &str, key: &str) -> Result<()> {
        let name = known(name)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(Error::EmptyApiKey);
        }
        self.secrets.set(name, key)
    }

    /// Removes a provider's key.
    pub fn delete_api_key(&self, name: &str) -> Result<()> {
        self.secrets.delete(known(name)?)
    }

    /// Every stored key, as the environment an agent other than Claude Code is started with.
    pub(crate) fn key_env(&self) -> Result<SecretEnv> {
        let mut env = vec![];
        for (name, _) in PROVIDERS {
            let key = self.secrets.get(name)?;
            env.extend(key.map(|key| ((*name).to_string(), key)));
        }
        Ok(SecretEnv(env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_environments_print_names_only() {
        let env = SecretEnv(vec![("OPENROUTER_API_KEY".into(), "sk-or-123".into())]);
        assert_eq!(format!("{env:?}"), r#"["OPENROUTER_API_KEY"]"#);
        let store = MemorySecrets::default();
        store.set("a", "sk-secret").expect("stores");
        assert!(!format!("{store:?}").contains("sk-secret"));
    }

    #[test]
    fn memory_secrets_set_replace_and_delete() {
        let store = MemorySecrets::default();
        assert_eq!(store.get("k").expect("reads"), None);
        store.set("k", "one").expect("stores");
        store.set("k", "two").expect("stores");
        assert_eq!(store.get("k").expect("reads").as_deref(), Some("two"));
        store.delete("k").expect("deletes");
        store.delete("k").expect("deleting nothing is fine");
        assert_eq!(store.get("k").expect("reads"), None);
    }
}
