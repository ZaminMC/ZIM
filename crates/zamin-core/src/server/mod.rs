//! Server identity and the registry (ADR-0004).

pub mod marker;
pub mod registry;

use std::fmt;

use crate::error::CoreError;

/// Immutable slug identifying a server: `^[a-z0-9][a-z0-9_-]{0,63}$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServerId(String);

impl ServerId {
    pub fn parse(raw: &str) -> Result<ServerId, CoreError> {
        let reason = match validation_error(raw) {
            Some(reason) => reason,
            None => return Ok(ServerId(raw.to_owned())),
        };
        Err(CoreError::InvalidServerId {
            id: raw.to_owned(),
            reason: reason.to_owned(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for ServerId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ServerId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        ServerId::parse(&raw).map_err(serde::de::Error::custom)
    }
}

fn validation_error(raw: &str) -> Option<&'static str> {
    if raw.is_empty() {
        return Some("it is empty");
    }
    if raw.len() > 64 {
        return Some("it is longer than 64 characters");
    }
    let first = raw.as_bytes()[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Some("it must start with a lowercase letter or digit");
    }
    if !raw
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Some("only lowercase letters, digits, '-' and '_' are allowed");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_ids() {
        for id in ["production", "a", "0", "dev-purpur-1_20", "s1_2-3"] {
            assert!(ServerId::parse(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_ids() {
        for id in [
            "",
            "-lead",
            "_lead",
            "UPPER",
            "has space",
            "dot.dot",
            "über",
            "path/like",
        ] {
            assert!(ServerId::parse(id).is_err(), "{id} should be invalid");
        }
        let long = "a".repeat(65);
        assert!(ServerId::parse(&long).is_err());
    }

    #[test]
    fn serializes_as_plain_string() {
        let id = ServerId::parse("production").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"production\"");
        assert_eq!(
            serde_json::from_str::<ServerId>("\"production\"").unwrap(),
            id
        );
    }
}
