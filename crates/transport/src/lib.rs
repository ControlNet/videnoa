//! Authenticated, fixed-service TCP tunnels over iroh.
mod identity;
mod tunnel;

pub use identity::Identity;
pub use iroh::{EndpointAddr, EndpointId};
pub use tunnel::{Authorizer, Client, PeerMap, Server, TunnelError};
