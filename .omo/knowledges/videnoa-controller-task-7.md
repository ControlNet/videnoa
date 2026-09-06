# Controller Task 7

- Task intake is mounted only by `controller_app_router`; legacy auth-only router behavior remains unchanged.
- Mutation authorization is centralized in `auth::boundary`: bearer requests are CSRF-exempt, while cookie sessions require same-origin and CSRF proof.
- Intake checks durable idempotency before validation or filesystem access, allowing safe replay after input drift or disappearance.
- New tasks persist descriptor-derived input identity as a nullable 16-byte BLOB so existing databases migrate without fabricating identity data.
- History queries use bounded offset pagination, escaped case-insensitive path search, deterministic ID tie-breaks, and explicit null-last sorting.
- Duration is defined only for completed tasks as `completed_at_ms - created_at_ms`; active rows sort last.
