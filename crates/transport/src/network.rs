//! Which relay and address-lookup infrastructure an endpoint uses.
use anyhow::{Context, Result};
use iroh::endpoint::{presets, Builder};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr};

/// Relays an endpoint connects through.
///
/// With no relays the endpoint uses the public N0 relays and address lookup.
/// With self-hosted relays it uses only those relays and no public address
/// lookup: it publishes nothing, and peers are dialed through the configured
/// relays, from where a direct path is still attempted. Worker and Controller
/// must then list the same relays.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Network {
    relays: Vec<RelayUrl>,
    /// Test relays use self-signed certificates.
    #[cfg(test)]
    insecure_relay_tls: bool,
}

impl Network {
    /// Uses only these relays (HTTP or HTTPS URLs); an empty list keeps the
    /// public N0 infrastructure.
    ///
    /// # Errors
    /// Returns an error when a URL does not parse or is not HTTP(S).
    pub fn with_relays<S: AsRef<str>>(urls: impl IntoIterator<Item = S>) -> Result<Self> {
        let mut relays: Vec<RelayUrl> = Vec::new();
        for url in urls {
            let url = url.as_ref().trim();
            let relay: RelayUrl = url
                .parse()
                .with_context(|| format!("invalid iroh relay URL: {url}"))?;
            anyhow::ensure!(
                matches!(relay.scheme(), "http" | "https"),
                "iroh relay URL must use http or https: {url}"
            );
            if !relays.contains(&relay) {
                relays.push(relay);
            }
        }
        Ok(Self {
            relays,
            #[cfg(test)]
            insecure_relay_tls: false,
        })
    }

    /// The configured relays; empty when the public N0 relays are used.
    #[must_use]
    pub fn relays(&self) -> &[RelayUrl] {
        &self.relays
    }

    pub(crate) fn builder(&self) -> Builder {
        if self.relays.is_empty() {
            return Endpoint::builder(presets::N0);
        }
        let builder = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::custom(self.relays.iter().cloned()));
        #[cfg(test)]
        let builder = if self.insecure_relay_tls {
            builder.ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify())
        } else {
            builder
        };
        builder
    }

    /// Adds the configured relays as dialing hints, since a self-hosted
    /// network has no address lookup to find the peer's relay.
    pub(crate) fn dial_address(&self, peer: EndpointAddr) -> EndpointAddr {
        peer.with_addrs(self.relays.iter().cloned().map(TransportAddr::Relay))
    }

    #[cfg(test)]
    pub(crate) fn with_insecure_test_relay_tls(mut self) -> Self {
        self.insecure_relay_tls = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_urls_are_validated_and_deduplicated() -> Result<()> {
        let network = Network::with_relays([
            "https://relay.example.test",
            " https://relay.example.test ",
            "http://127.0.0.1:3340",
        ])?;
        assert_eq!(
            network
                .relays()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["https://relay.example.test/", "http://127.0.0.1:3340/"]
        );
        assert!(Network::with_relays(["not a url"]).is_err());
        assert!(Network::with_relays(["ftp://relay.example.test"]).is_err());
        assert!(Network::with_relays(Vec::<String>::new())?
            .relays()
            .is_empty());
        Ok(())
    }

    #[test]
    fn self_hosted_dialing_hints_every_configured_relay() -> Result<()> {
        let network = Network::with_relays(["https://a.example.test", "https://b.example.test"])?;
        let id = iroh::SecretKey::generate().public();
        let address = network.dial_address(EndpointAddr::new(id));
        assert_eq!(address.id, id);
        assert_eq!(address.relay_urls().count(), 2);
        assert_eq!(address.ip_addrs().count(), 0);
        // The public network relies on address lookup instead.
        let public = Network::default().dial_address(EndpointAddr::new(id));
        assert!(public.is_empty());
        Ok(())
    }
}
