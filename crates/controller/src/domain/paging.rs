use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PageLimit(u16);

impl PageLimit {
    pub const DEFAULT: u16 = 100;
    pub const MAXIMUM: u16 = 500;

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PageOffset(u64);

impl PageOffset {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    limit: PageLimit,
    offset: PageOffset,
}

#[derive(Debug, thiserror::Error)]
pub enum PagingError {
    #[error("page limit must be greater than zero")]
    ZeroLimit,
    #[error("page limit {value} exceeds maximum {maximum}")]
    LimitTooLarge { value: u64, maximum: u16 },
    #[error("page offset must be nonnegative")]
    NegativeOffset,
}

impl PageRequest {
    /// Creates a bounded page request using the locked default when `limit` is omitted.
    ///
    /// # Errors
    /// Returns [`PagingError`] for zero or oversized limits and negative offsets.
    pub fn try_new(limit: Option<u64>, offset: i64) -> Result<Self, PagingError> {
        let limit = limit.unwrap_or(u64::from(PageLimit::DEFAULT));
        if limit == 0 {
            return Err(PagingError::ZeroLimit);
        }
        if limit > u64::from(PageLimit::MAXIMUM) {
            return Err(PagingError::LimitTooLarge {
                value: limit,
                maximum: PageLimit::MAXIMUM,
            });
        }
        let offset = u64::try_from(offset).map_err(|_| PagingError::NegativeOffset)?;
        let limit = u16::try_from(limit).map_err(|_| PagingError::LimitTooLarge {
            value: limit,
            maximum: PageLimit::MAXIMUM,
        })?;
        Ok(Self {
            limit: PageLimit(limit),
            offset: PageOffset(offset),
        })
    }

    #[must_use]
    pub const fn limit(self) -> PageLimit {
        self.limit
    }

    #[must_use]
    pub const fn offset(self) -> PageOffset {
        self.offset
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: PageLimit(PageLimit::DEFAULT),
            offset: PageOffset(0),
        }
    }
}

impl<'de> Deserialize<'de> for PageRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPageRequest {
            #[serde(default)]
            limit: Option<u64>,
            #[serde(default)]
            offset: i64,
        }

        let raw = RawPageRequest::deserialize(deserializer)?;
        Self::try_new(raw.limit, raw.offset).map_err(serde::de::Error::custom)
    }
}
