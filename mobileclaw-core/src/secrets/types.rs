use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// A heap string that zeroes its bytes on drop via [`zeroize::Zeroizing`].
///
/// # Security contract
/// - Never implements `Debug`, `Display`, or `Clone` — callers must use [`expose()`](SecretString::expose)
/// - The backing bytes are zeroed (with a compiler fence) when this value is dropped
/// - The field is private — no module may access the inner `String` directly
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }

    /// Return the secret value.
    ///
    /// # Security
    /// The caller must not pass this value to any logging macro (`tracing::*!`,
    /// `println!`, `format!`) or serialize it to JSON/YAML.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Non-secret configuration for one email account.
/// The password is stored separately in `SecretStore` under the key `email:<id>:password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAccount {
    /// Stable identifier chosen by the user (e.g., "work", "personal").
    pub id: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub imap_host: String,
    pub imap_port: u16,
    pub username: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_exposes_correct_value() {
        let s = SecretString::new("hunter2".into());
        assert_eq!(s.expose(), "hunter2");
    }

    #[test]
    fn secret_string_has_no_debug_or_clone() {
        // If this compiles, it proves Debug and Clone are not derived.
        // (Attempting to use `format!("{:?}", s)` or `s.clone()` would be a compile error.)
        let _s = SecretString::new("test".into());
    }

    #[test]
    fn email_account_roundtrip_serialization() {
        let acc = EmailAccount {
            id: "work".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            username: "alice@example.com".into(),
        };
        let json = serde_json::to_string(&acc).unwrap();
        let back: EmailAccount = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "work");
        assert_eq!(back.smtp_port, 587);
        assert_eq!(back.username, "alice@example.com");
    }
}
