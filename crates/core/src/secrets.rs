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
    /// Whether a secret is stored under `name`. A store that can tell without reading the
    /// secret does so, since on macOS reading is what the user gets asked about.
    fn has(&self, name: &str) -> Result<bool> {
        Ok(self.get(name)?.is_some())
    }
}

/// A store that reads each secret from `inner` at most once for as long as the app runs.
///
/// Every read of the keychain can put a password prompt in front of the user, so what was
/// read is kept in memory, where it would end up anyway: in the environment of the agents.
/// Whoever else changes the keychain meanwhile is noticed the next time the app starts.
pub struct Remembered<S> {
    inner: S,
    read: Mutex<BTreeMap<String, Option<String>>>,
}

impl<S> Remembered<S> {
    /// Wraps `inner`, having read nothing yet.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            read: Mutex::new(BTreeMap::new()),
        }
    }
}

impl<S> fmt::Debug for Remembered<S> {
    /// Never prints the secrets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Remembered(<redacted>)")
    }
}

impl<S: SecretStore> SecretStore for Remembered<S> {
    fn get(&self, name: &str) -> Result<Option<String>> {
        if let Some(known) = lock(&self.read).get(name) {
            return Ok(known.clone());
        }
        let value = self.inner.get(name)?;
        lock(&self.read).insert(name.to_string(), value.clone());
        Ok(value)
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        self.inner.set(name, value)?;
        lock(&self.read).insert(name.to_string(), Some(value.to_string()));
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.inner.delete(name)?;
        lock(&self.read).insert(name.to_string(), None);
        Ok(())
    }

    fn has(&self, name: &str) -> Result<bool> {
        if let Some(known) = lock(&self.read).get(name) {
            return Ok(known.is_some());
        }
        self.inner.has(name)
    }
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
    use security_framework::item::{ItemClass, ItemSearchOptions};
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

        /// Looks the entry up by its attributes alone. The secret is not read, so macOS has
        /// nothing to ask the user about.
        fn has(&self, name: &str) -> Result<bool> {
            let mut search = ItemSearchOptions::new();
            search
                .class(ItemClass::generic_password())
                .service(SERVICE)
                .account(name)
                .load_attributes(true);
            Ok(found(search.search())?.is_some_and(|entries| !entries.is_empty()))
        }
    }
}

/// Where API keys are kept on this platform.
#[must_use]
pub fn platform_store() -> Box<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    return Box::new(Remembered::new(Keychain));
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
                stored: self.secrets.has(name)?,
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

    /// Counts how often it is read, and holds one secret.
    #[derive(Debug, Default)]
    struct Counting {
        reads: Mutex<u32>,
        held: MemorySecrets,
    }

    impl SecretStore for Counting {
        fn get(&self, name: &str) -> Result<Option<String>> {
            *lock(&self.reads) += 1;
            self.held.get(name)
        }

        fn set(&self, name: &str, value: &str) -> Result<()> {
            self.held.set(name, value)
        }

        fn delete(&self, name: &str) -> Result<()> {
            self.held.delete(name)
        }

        /// Told without reading, as the keychain can.
        fn has(&self, name: &str) -> Result<bool> {
            Ok(lock(&self.held.0).contains_key(name))
        }
    }

    #[test]
    fn a_secret_is_read_from_the_store_once_and_never_to_see_whether_it_is_there() {
        let inner = Counting::default();
        inner.set("a", "sk-one").expect("stores");
        let store = Remembered::new(inner);
        let reads = |store: &Remembered<Counting>| *lock(&store.inner.reads);

        // Asking whether it is there reads nothing.
        assert!(store.has("a").expect("looks"));
        assert!(!store.has("b").expect("looks"));
        assert_eq!(reads(&store), 0);
        // Reading it, and reading that another is missing, happens once each.
        for _ in 0..3 {
            assert_eq!(store.get("a").expect("reads").as_deref(), Some("sk-one"));
            assert_eq!(store.get("b").expect("reads"), None);
        }
        assert_eq!(reads(&store), 2);
        // What is written or removed through it is known without reading again.
        store.set("b", "sk-two").expect("stores");
        store.delete("a").expect("deletes");
        assert_eq!(store.get("b").expect("reads").as_deref(), Some("sk-two"));
        assert_eq!(store.get("a").expect("reads"), None);
        assert!(!store.has("a").expect("looks"));
        assert_eq!(reads(&store), 2);
        assert!(!format!("{store:?}").contains("sk-"));
    }

    /// Touches the real keychain, so it is run by hand: `cargo test -- --ignored keychain`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes to the login keychain of whoever runs it"]
    fn the_keychain_says_whether_an_entry_is_there_without_reading_it() {
        let name = "swarmed-test-entry";
        Keychain.delete(name).expect("deletes");
        assert!(!Keychain.has(name).expect("looks"));
        Keychain.set(name, "nothing secret").expect("stores");
        assert!(Keychain.has(name).expect("looks"));
        Keychain.delete(name).expect("deletes");
        assert!(!Keychain.has(name).expect("looks"));
    }

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
