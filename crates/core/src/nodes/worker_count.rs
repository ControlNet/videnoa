use std::num::NonZeroUsize;

use anyhow::{bail, Context, Result};

use crate::types::PortData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkerCount(NonZeroUsize);

impl WorkerCount {
    pub(crate) const ONE: Self = Self(NonZeroUsize::MIN);

    pub(crate) fn parse(value: &PortData) -> Result<Self> {
        let PortData::Int(value) = value else {
            bail!("num_workers must be an Int");
        };
        let value = usize::try_from(*value).context("num_workers must be positive")?;
        let value = NonZeroUsize::new(value).context("num_workers must be positive")?;
        Ok(Self(value))
    }

    pub(crate) const fn get(self) -> usize {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_any_positive_integer() {
        let workers = WorkerCount::parse(&PortData::Int(7)).expect("seven workers should parse");

        assert_eq!(workers.get(), 7);
    }

    #[test]
    fn parse_rejects_zero() {
        let error = WorkerCount::parse(&PortData::Int(0)).expect_err("zero workers should fail");

        assert!(error.to_string().contains("num_workers must be positive"));
    }
}
