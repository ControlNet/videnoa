# HTTP header read timeout (slow-loris)

- `axum::serve` (axum 0.8.8) builds `hyper_util::server::conn::auto::Builder`
  without a timer. hyper 1.x only applies its default 30 s HTTP/1
  `header_read_timeout` through `Timer::check`, so with no timer the deadline
  never fires and a peer can dribble header bytes forever.
- Fix: production servers use `HttpServer` instead of `axum::serve`:
  - Worker (app, desktop): `videnoa_core::server::http_server`.
  - Controller: `videnoa_controller::http_server` (the Controller does not
    depend on core).
  Both mirror axum's accept loop (accept-error backoff of 1 s for non-transient
  errors), set `http1().timer(TokioTimer::new()).header_read_timeout(..)`, use
  `serve_connection_with_upgrades` (WebSockets), insert
  `ConnectInfo<SocketAddr>` per request, and drain with
  `hyper_util::server::graceful::GracefulShutdown`.
- hyper starts the head deadline only while reading a request head (also the
  next head on an idle keep-alive connection). Bodies, slow handlers, SSE and
  upgraded sockets are not bounded.
- Not covered: HTTP/2 prior-knowledge (h2c) connections, which hyper-util's
  auto builder still accepts, as before. The iroh loopback servers still use
  `axum::serve`; they only accept local tunnel traffic.
- Tests: `crates/core/tests/http_server.rs`,
  `crates/controller/tests/http_server.rs` (200 ms override via
  `HttpServer::header_read_timeout`).
