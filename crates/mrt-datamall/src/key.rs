//! The DataMall account key.

use crate::error::DataMallError;

/// The name of the environment variable that holds the account key.
pub const ACCOUNT_KEY_ENV: &str = "LTA_DATAMALL_ACCOUNT_KEY";

/// An LTA DataMall account key.
///
/// LTA issues the key when you register at DataMall. The key is a
/// secret. Keep these rules:
///
/// - Do not write the key into source code.
/// - Do not commit the key to the repository.
/// - Supply the key at run time, for example through the
///   `LTA_DATAMALL_ACCOUNT_KEY` environment variable.
///
/// The `Debug` output of this type does not show the key.
#[derive(Clone)]
pub struct AccountKey(String);

impl AccountKey {
    /// Make a key from a string.
    ///
    /// The function trims surrounding whitespace. It returns an error
    /// if the trimmed key is empty.
    pub fn new(key: impl Into<String>) -> Result<Self, DataMallError> {
        let key = key.into().trim().to_string();
        if key.is_empty() {
            return Err(DataMallError::EmptyKey);
        }
        Ok(AccountKey(key))
    }

    /// Read the key from the `LTA_DATAMALL_ACCOUNT_KEY` environment
    /// variable.
    pub fn from_env() -> Result<Self, DataMallError> {
        match std::env::var(ACCOUNT_KEY_ENV) {
            Ok(value) => AccountKey::new(value),
            Err(_) => Err(DataMallError::MissingEnv(ACCOUNT_KEY_ENV)),
        }
    }

    /// Get the key value for the request header.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountKey(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_key() {
        let key = AccountKey::new("super-secret-value").unwrap();
        let debug = format!("{key:?}");
        assert!(!debug.contains("super-secret-value"));
        assert_eq!(debug, "AccountKey(redacted)");
    }

    #[test]
    fn empty_keys_are_rejected() {
        assert!(matches!(AccountKey::new(""), Err(DataMallError::EmptyKey)));
        assert!(matches!(
            AccountKey::new("   "),
            Err(DataMallError::EmptyKey)
        ));
    }

    #[test]
    fn keys_are_trimmed() {
        let key = AccountKey::new("  abc123  ").unwrap();
        assert_eq!(key.as_str(), "abc123");
    }
}
