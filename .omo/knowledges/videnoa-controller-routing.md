# Controller Routing Knowledge

- In Axum 0.8, `/api/{*path}` matches non-empty descendants such as `/api/jobs`, but not `/api` or `/api/`.
- Register `/api`, `/api/`, and `/api/{*path}` explicitly before the Controller SPA fallback when every unknown API method must return JSON rather than HTML.
- Run Controller HTTP contracts in both debug and release profiles because debug uses `ServeDir`/`ServeFile`, while release uses embedded fallback handling with different method behavior.
