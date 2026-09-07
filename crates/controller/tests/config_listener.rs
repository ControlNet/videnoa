use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::routing::get;
use axum::Router;
use tokio_util::sync::CancellationToken;
use videnoa_controller::config::{listener_channel, serve_reconfigurable, PreparedListener};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn handoff_closes_old_generation_event_streams() -> TestResult {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let initial = PreparedListener::bind(address).await?;
    let initial_address = initial.address();
    let replacement = PreparedListener::bind(address).await?;
    let (handle, receiver) = listener_channel();
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(serve_reconfigurable(
        initial,
        Router::new().route(
            "/events",
            get(|| async {
                use futures_util::{stream, StreamExt};
                let initial = stream::once(async {
                    Ok::<_, std::convert::Infallible>(
                        axum::response::sse::Event::default().data("ready"),
                    )
                });
                axum::response::Sse::new(initial.chain(stream::pending()))
            }),
        ),
        receiver,
        shutdown.clone(),
    ));
    let mut response = reqwest::get(format!("http://{initial_address}/events")).await?;
    assert!(response.chunk().await?.is_some());
    handle.handoff(replacement).await?;
    let ended = tokio::time::timeout(std::time::Duration::from_secs(2), response.chunk()).await;
    shutdown.cancel();
    server.abort();
    assert!(ended??.is_none());
    Ok(())
}

#[tokio::test]
async fn prebound_listener_handoff_serves_the_same_router_without_restart() -> TestResult {
    // Given: a running router and a second listener already bound on a new address.
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let initial = PreparedListener::bind(SocketAddr::new(loopback, 0)).await?;
    let initial_address = initial.address();
    let replacement = PreparedListener::bind(SocketAddr::new(loopback, 0)).await?;
    let replacement_address = replacement.address();
    let (handle, receiver) = listener_channel();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(serve_reconfigurable(
        initial,
        Router::new().route("/probe", get(|| async { "ready" })),
        receiver,
        server_shutdown,
    ));
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{initial_address}/probe"))
            .send()
            .await?
            .text()
            .await?,
        "ready"
    );

    // When: the prepared listener is handed to the running server loop.
    handle.handoff(replacement).await?;

    // Then: the new address immediately serves the shared application router.
    assert_eq!(
        client
            .get(format!("http://{replacement_address}/probe"))
            .send()
            .await?
            .text()
            .await?,
        "ready"
    );
    shutdown.cancel();
    server.await??;
    Ok(())
}
