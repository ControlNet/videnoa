# Endpoint ID registration direction

## User preference

Deploy Videnoa, obtain the worker's iroh Endpoint ID (explicitly not a ticket),
then add a worker in Controller by entering that Endpoint ID.

## Confirmed follow-up decisions

- Expose the worker iroh switch in the config file and WebUI settings; display
  its Endpoint ID in that settings page. Identity remains persisted and private
  key material is never displayed.
- Reuse the worker password to authorize each new tunnel before TCP forwarding.
  No password means no iroh forwarding. Removing the password closes existing
  tunnels, stops iroh, and persists its switch as disabled; setting a password
  later does not automatically re-enable it. Changing the password preserves
  existing tunnels and requires the new password for new tunnels. Existing HTTP
  request authentication remains in effect.
- Iroh primarily exposes worker APIs, not the worker WebUI. Password changes
  require the user to manually edit the saved password in Controller; no extra
  password synchronization/update workflow is requested.

- Use N0's default relay and address discovery for the first implementation.
  Self-hosted relay support is deferred.
- Persist iroh private identities under the application's existing persistent
  directory alongside SQLite/config storage, using separate identity files.
  Reuse identities across restarts and preserve them with the persistent volume.
- Another agent is implementing worker password authentication. Wait for that
  work to finish, then rebase this feature worktree onto its completed changes
  and implement iroh using that password authentication mechanism.
- The user has authorized that subsequent rebase; its target branch/commit is
  not yet supplied. Do not rebase onto an assumed or unfinished target.
- The Controller Endpoint ID allowlist below was an earlier proposal, not a
  requirement. Password authentication is the selected integration direction;
  do not implement a competing authentication mechanism in this worktree.
- Inspect the completed authentication implementation before deciding how it
  protects HTTP routes and tunnel access. The existing fixed-target forwarding
  boundary still applies; password-protected HTTP does not itself specify proxy
  CONNECT authorization.

## Assessment

This is compatible with TCP forwarding. Videnoa should manage the forwarding
lifecycle and local listener internally so users register the actual remote
identity, not an ephemeral local proxy URL. Existing worker request/response
types currently require `api_url: WorkerApiUrl`, which validates HTTP(S) only;
native registration therefore needs an explicit transport/endpoint model and
persistence changes while retaining existing HTTP registrations.

ID-only dialing requires working address publication and lookup. Iroh 1.1.0's
N0 preset configures the default relays and publication/resolution through
iroh.link. Endpoint IDs identify public keys; the underlying private identity
must persist across worker and Controller restarts. Private runtime files must
remain outside git (with .gitignore coverage if stored under the checkout).

Earlier authorization proposal (superseded by the password integration decision):
explicitly allow the Controller's Endpoint ID on the
worker before forwarding is permitted. An alternative is a worker-side pairing
approval with independently verified Controller identity. Neither is an accepted
user decision yet. Do not authorize the first unknown connection automatically.
Knowing the worker's public Endpoint ID establishes the connection target, not
permission to submit jobs or access files.

Proposed operational states distinguish address lookup failure, unreachable
endpoint, denied authorization, and unhealthy worker API. A reachable relay is
not sufficient evidence of application readiness. Restrict forwarding to the
configured worker API target.

## Sources verified on 2026-09-07

- https://docs.rs/iroh/1.1.0/iroh/endpoint/presets/struct.N0.html
- https://docs.rs/iroh/1.1.0/iroh/endpoint/struct.Endpoint.html
- https://docs.rs/iroh/1.1.0/iroh/
- `crates/controller/src/domain/worker.rs`
- `crates/controller/src/domain/values.rs`

Documentation only; no implementation or network validation performed.
