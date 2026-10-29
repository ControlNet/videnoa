# Review after v0.1.10 (2026-10-29, at a883a9b)

## How the review ran

- Codex (gpt-6-astra) ran two read-only reviews. They covered the surface
  that earlier reviews had covered only lightly:
  - transport, worker HTTP server security, app and desktop;
  - CI and release, packaging, Docker, the worker web UI.
- Claude subagents verified every finding, adding a failing test where
  possible. The reproduction tests sit in the agent worktrees under
  `.claude/worktrees/` and are not committed.
- From now on reviews use Claude subagents rather than Codex: the user's
  Codex quota is nearly used up.
- Local checks:
  - `cargo deny check advisories`: clean.
  - npm audit for `web` and `controller-web`: 6 advisories each, all fixable
    within the current semver ranges with `npm audit fix`.
    - The only one that ships in a production bundle is zod (GHSA-mrhx-c7xc-m3w5,
      memory use when validating oversized collections). It only parses
      responses from our own server.
    - vite, vitest, undici and source-map-js are dev and build tools only.

## Verified findings

### Transport and worker server

1. **Confirmed by test: an unauthenticated CONNECT can hold a tunnel stream
   permit forever.**
   - Where: `crates/transport/src/tunnel.rs:229-233` and `:250`.
   - The rejection write and the success write both run outside
     `HANDSHAKE_TIMEOUT`.
   - A peer can set `stream_receive_window(0)`, which iroh exposes through
     `QuicTransportConfig`. The server then can never write those bytes,
     and each stuck task keeps one of the 256 permits (`:104`, `:135`).
   - Once every permit is held, new streams are dropped (`:129`), including
     the controller's. No password is needed, only the Endpoint ID.
   - Fix: wrap both writes in `timeout(HANDSHAKE_TIMEOUT, …)`.
2. **Confirmed by test: the Jellyfin client forwards `X-Emby-Token` across
   cross-host redirects.**
   - Where: `crates/core/src/jellyfin.rs:83-92`. The token is a default
     header, and reqwest's default redirect policy is used.
   - reqwest strips only standard auth headers on a cross-host or cross-port
     hop (`redirect.rs:239-250`), not a custom header like this one.
   - The caller supplies both the URL and the key in the query of
     `/api/jellyfin/*`. So the leak happens only when the Jellyfin host
     itself redirects somewhere else.
   - Fix: use `redirect::Policy::none()`, or a policy that drops the token
     on a cross-host hop.

### Release and packaging

3. **Confirmed by reading: an existing release tag can receive another
   commit's artifacts.**
   - Where: `.github/workflows/release.yaml:112-129` and `:514-516`.
   - Two triggers:
     - `force_publish` never compares the existing tag with `GITHUB_SHA`;
     - any non-zero `git ls-remote --exit-code` result (128 on a network
       error) is treated as "tag absent".
   - Docker `latest`/version tags and the release assets are overwritten,
     and the verify job only pulls the images, so nothing reports it.
   - Neither trigger can happen in the documented GitFlow flow. The impact
     is silent divergence of tag, assets and images.
   - Fix, in the version gate:
     - return code 2 means absent, 0 means present, anything else fails the
       run;
     - with force, require the tag's peeled commit (`^{}`; the tags are
       annotated) to equal `GITHUB_SHA`.
4. **Confirmed by reading: the published Docker images are rebuilt rather
   than the tested ones.**
   - The unittest smoke builds and `build-push-action` in the release
     workflow are separate builds with no hand-off.
   - Base images are tags, not digests. Builder apt and runtime
     `ca-certificates curl` are unpinned.
   - In run 40049339005 publication ran about an hour after the smoke tests.
   - Fix: in each publish job, build with `load: true`, run the existing
     image checks, then push from the same builder (cached). Keep
     `build-push-action` so the provenance attestation is kept.
5. **Confirmed by reading: licence notices are missing.**
   - Docker drops TensorRT's `LICENSE.txt` (`Dockerfile:82-87`, `:149`) and
     ONNX Runtime's `LICENSE` and `ThirdPartyNotices.txt` (`Dockerfile:71-74`).
   - The `misc` `lib_*.zip` bundles contain only libraries: no TensorRT,
     ONNX Runtime, CUDA or cuDNN notices.
   - The media tools bundle is fine.
   - Open question for the user, not judged: TensorRT licence section 12.1
     names only `libnvinfer` and `libnvinfer_plugin` as redistributable. We
     also ship `libnvonnxparser` and `libnvinfer_builder_resource`.

### Worker web UI (`web/`), each reproduced with Vitest

6. **Medium.** Deleting a saved workflow happens on the first click, with no
   confirmation. See `PresetGallery.tsx:126-138` and `:211`; the server runs
   `remove_file`.
   - Job-history delete also has no confirm step.
   - The pattern to reuse is the Dialog in `web/src/auth/AccessSettings.tsx:49`.
7. **Low to Medium.** Cancel, retry and history-delete failures are
   swallowed. `JobsPage.tsx:95`, `:232` and `:243` have empty catch blocks,
   and the store sets no error state.
   - A failed Cancel leaves the GPU job running with no message.
8. **Low.** A Print preview can show on another workflow's node.
   - Cause: the editor tracks the first running job globally
     (`EditorPage.tsx:127-133`), and previews are matched by node id only
     (`CustomNode.tsx:580`).
   - It needs matching node ids: palette nodes use uuidv4, and no built-in
     preset has a Print node.
9. **Trivial.** Previews of deleted jobs stay in `runtimePreviewsByJobId`.
   Clear them in `deleteJobHistory`, but not on socket close, because the
   Print output panel of finished jobs relies on the map.

## Fixes (2026-10-29)

The user chose to fix 1, 2, 6, 7, 8, 9 and the npm advisories. Each fix has
a test that failed on the old code.

- **1 (`6fe2f14`).**
  - `respond()` in `tunnel.rs` writes both CONNECT responses within
    `HANDSHAKE_TIMEOUT` and resets the stream on expiry.
  - Test: `tunnel::permit_exhaustion_tests::rejected_connects_to_a_zero_window_peer_release_their_stream_permits`.
    A legitimate tunnel recovers within about 10 s while 320 zero-window
    streams stay open.
  - What is still possible: a peer that keeps opening slow handshakes can
    still occupy permits for up to 10 s each. This is ordinary slowloris
    exposure. Finding 7 (connection slots, cached connections) is unchanged
    on purpose.
- **2 (`a13b148`).**
  - `same_origin_redirects` follows a redirect only within the same origin,
    or from http to https on the same host. Any other redirect fails with
    only the target host named.
  - Tests are in `jellyfin::tests`.
- **npm (`bd94a2a`).** `npm audit fix` changed only the lockfiles. Both apps
  now report 0 vulnerabilities.
- **Web 6-9**: done by a subagent, see `web-ui-fixes-bugs-6-9-2026-10-29.md`.
  - `6d2001b`: previews are dropped when job history is deleted.
  - `32d7eba`: failures show toasts; a 401 shows none.
  - `c930448`: deleting a workflow needs confirmation; the editor's Escape
    respects `defaultPrevented`.
  - `8c0b251`: the editor follows only the job its canvas submitted
    (`submittedJobId`).

Not fixed, by the user's choice:
- 3: release tag guard;
- 4: Docker images rebuilt between smoke test and publish;
- 5: licence notices, plus the TensorRT 12.1 question.
