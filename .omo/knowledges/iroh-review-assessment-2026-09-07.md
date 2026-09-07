# Follow-up review assessment — 2026-09-07

Read-only code/dependency review of local feature/iroh at
`af3af998ed9497b9cd60caa64711c5ce7677ec57`; no implementation changes or Windows CI
reproduction were performed in this assessment. This supplements the initial
implementation record with two confirmed gaps.

- `workers/health/probe.rs` classifies every health client error as Health, while
  only capability errors classify ClientStatus 401/403 as Authentication. Iroh
  authenticates CONNECT before sending the public health request, so a wrong
  saved credential bypasses the existing version-based authentication block.
  Both request stages should share 401/403 detection; other failure categories
  should retain their current behavior. Tests must observe admission attempts,
  not just origin HTTP requests, since failed admission never reaches HTTP.
- Windows-target cargo tree shows iroh 1.1.0 -> netwatch 0.19.3 -> wmi 0.18.4
  (also iroh -> portmapper 0.19.3 -> netwatch). Wmi currently resolves windows
  0.61.3 -> windows-core 0.61.2 -> windows-result 0.3.4, while its direct
  windows-core dependency resolves 0.62.2 -> windows-result 0.4.1. Netwatch itself
  uses windows 0.62.2. Wmi's published manifest permits both windows and
  windows-core independently in >=0.59,<0.63, allowing inconsistent families.
  Multiple versions across a graph alone are valid; crossing these types inside
  wmi is the issue. A narrow alignment of wmi's resolutions should be evaluated
  before changing iroh or upgrading unrelated dependencies.

The proposed follow-up scope is appropriate. Actual Windows Rust CI must confirm
any fix; Linux-to-MSVC cargo check may require native tooling and does not prove
Windows runtime behavior. Existing authentication blocks are in-memory and tied
to the persisted health-update version; do not accidentally add restart-persistent
blocking or assert exactly one version increment across both edit and health writes.
