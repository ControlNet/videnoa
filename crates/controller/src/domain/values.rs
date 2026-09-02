use std::fmt;
use std::num::NonZeroU16;

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

macro_rules! string_value {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }
    };
}

string_value!(InputPath);
string_value!(OutputPath);
string_value!(InputExtension);
string_value!(OutputExtension);
string_value!(RemotePath);
string_value!(WorkflowName);
string_value!(SourceReference);
string_value!(WorkerName);
string_value!(IdempotencyKey);

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([redacted])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WorkerApiUrl(Url);

#[derive(Debug, thiserror::Error)]
pub enum WorkerUrlError {
    #[error("worker API URL is malformed")]
    Malformed(#[source] url::ParseError),
    #[error("worker API URL must use http or https")]
    UnsupportedScheme,
    #[error("worker API URL must not contain credentials")]
    Credentials,
    #[error("worker API URL must not contain a query or fragment")]
    QueryOrFragment,
}

impl WorkerApiUrl {
    /// Parses a credential-free HTTP(S) base URL into its canonical trailing-slash form.
    ///
    /// # Errors
    /// Returns [`WorkerUrlError`] for malformed or policy-invalid URL values.
    pub fn parse(value: &str) -> Result<Self, WorkerUrlError> {
        let mut url = Url::parse(value).map_err(WorkerUrlError::Malformed)?;
        match url.scheme() {
            "http" | "https" => {}
            _ => return Err(WorkerUrlError::UnsupportedScheme),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(WorkerUrlError::Credentials);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(WorkerUrlError::QueryOrFragment);
        }
        let normalized = format!("{}/", url.path().trim_end_matches('/'));
        url.set_path(&normalized);
        Ok(Self(url))
    }

    #[must_use]
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkerApiUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

macro_rules! positive_u16 {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU16);

        impl $name {
            pub const fn get(self) -> u16 {
                self.0.get()
            }
        }

        impl TryFrom<u64> for $name {
            type Error = &'static str;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                let narrowed = u16::try_from(value).map_err(|_| "value exceeds u16")?;
                NonZeroU16::new(narrowed)
                    .map(Self)
                    .ok_or("value must be greater than zero")
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::try_from(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

positive_u16!(ComputeSlots);
positive_u16!(ConcurrencyLimit);
