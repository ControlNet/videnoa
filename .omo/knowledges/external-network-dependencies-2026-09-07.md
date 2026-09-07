# External network dependency audit

## Scope and limits

Inspected both production frontends, Rust runtime source, pinned iroh 1.1.0 / iroh-dns 1.1.0 dependency source, Dockerfiles, packaging scripts, release workflows, and workflow presets. This is source inspection with public reachability evidence, not a packet capture or a mainland-China/NAS connectivity test. Test fixtures, design HTML, schemas, documentation links, and localhost development proxies are not production runtime services.

## Findings

| Dependency | Trigger and impact | Evidence |
| --- | --- | --- |
| Google Fonts: `fonts.googleapis.com`, `fonts.gstatic.com` | Both frontends load Manrope and Geist Mono from the browser. A pending stylesheet can delay rendering; missing fonts use fallbacks. | `web/index.html`, `controller-web/index.html` |
| Public iroh discovery: `dns.iroh.link`, HTTPS `/pkarr`, endpoint DNS records | Worker and Controller transport use the N0 preset. Endpoint-ID-only connections use public discovery to find peers. | `crates/transport/src/tunnel.rs`, upstream `endpoint/presets.rs` and `address_lookup/pkarr.rs` |
| Public iroh relays: `use1-1.relay.n0.iroh.link`, `usw1-1.relay.n0.iroh.link`, `euc1-1.relay.n0.iroh.link`, `aps1-1.relay.n0.iroh.link` | Connection establishment/fallback uses N0 infrastructure. When direct connectivity is unavailable, relays carry encrypted traffic. No mainland reachability conclusion was established. | Pinned iroh `src/defaults.rs` |
| Google public DNS fallback | iroh-dns first uses system DNS configuration. If reading that configuration fails, it falls back to Hickory's Google UDP/TCP resolver set, including `8.8.8.8` and `8.8.4.4`. This is not the normal configured-system-DNS path, nor a fallback triggered by every failed lookup. | Pinned iroh-dns `HickoryResolver::build_resolver` |
| Hugging Face model URL | The built-in RealESRGAN model entry points to `huggingface.co`. A download method exists, but no production caller was found; the only direct invocation found was an ignored download test. Local inference does not inherently require this service. | `crates/core/src/model_registry.rs` |
| GitHub releases and repository | Packaging scripts download bundles and clone source. Worker Docker build fetches ONNX Runtime from Microsoft GitHub releases. App source links require GitHub only when followed. No automatic application update check was found. | `scripts/package_dist.sh`, `scripts/package_dist.ps1`, `Dockerfile`, both about/source-link implementations |
| Docker Hub | Published Worker/Controller images and unqualified build base images use Docker Hub. Affects pull/build/update, not an already-installed container's normal local processing. | `README.md`, `.github/workflows/release.yaml`, both Dockerfiles |
| Build package sources | Docker builds run npm, Cargo, apt, and (Worker) pip for TensorRT. These package registries are build-time dependencies; no blanket mainland blocking claim is made. | Both Dockerfiles, lockfiles, toolchain configuration |
| User-selected services | Jellyfin, HTTP/download nodes, stream input/output, and Worker API addresses connect to configured endpoints. They introduce only the destinations selected by users/workflows. | `crates/core/src/jellyfin.rs`, `crates/core/src/nodes/`, Controller remote clients |

The Controller initializes its iroh client lazily; Worker iroh is optional. An HTTP-only deployment with Worker iroh disabled does not need the N0 connection path. Local processing with models and libraries already present does not need Google, Hugging Face, or GitHub business APIs. Frontend API/SSE/WebSocket requests target the application's origin. No production analytics, reCAPTCHA, remote JS CDN, or external icon CDN was found in the inspected source.

## Mainland reachability evidence checked on 2026-09-07

- [GreatFire: Google Fonts](https://en.greatfire.org/https/fonts.googleapis.com): two recent conclusive tests connected; the displayed last test was 2026-08-20. Do not equate all Google services with a confirmed Fonts block, or generalize this small sample to every font-file URL and ISP.
- [GreatFire: Hugging Face](https://en.greatfire.org/domain/huggingface.co): blocked tested URLs, last domain test 2026-09-06.
- [GreatFire: GitHub](https://en.greatfire.org/https/github.com): mixed/interfered reachability, last test 2026-09-06. Does not establish identical behavior for every release asset/CDN URL.
- [GreatFire: Docker registry](https://en.greatfire.org/https/registry-1.docker.io): blocking recorded on 2026-04-18, but no tests in the last 90 days. Treat as historical risk, not proof of current failure on all networks.
- [Iroh FAQ](https://docs.iroh.computer/about/faq): public relay defaults and support for custom/self-hosted infrastructure. Exact defaults above come from this repository's pinned dependency source.

## Recommended order

1. Bundle licensed fonts locally to remove the unavoidable browser third-party dependency.
2. Provide offline release/model bundles and an operator-controlled image distribution option for mainland users.
3. Test N0 DNS/discovery/relay connectivity from the actual NAS and GPU networks. If necessary, expose custom relay and discovery settings together; changing relays alone does not remove public discovery dependence. Upstream supports customization, but the current application has no equivalent user-facing relay/discovery configuration.
4. Ensure valid system DNS in containers; consider making the transport resolver configurable rather than relying on Google's emergency fallback.

No runtime behavior or deployment was changed by this audit. Findings should not be read as proof that the previously observed relay Ping timeout was caused by geographic blocking.
