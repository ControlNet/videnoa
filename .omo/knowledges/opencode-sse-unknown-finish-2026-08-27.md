# OpenCode silent SSE termination investigation (2026-08-27)

## Scope

- Repository: `videnoa`
- Primary session: `ses_fbda71076ffe1Krfu4sIgGnltY`
- Comparison session: `ses_fbda11445ffewvQWsWdlmju2M0`
- OpenCode version: `1.17.18`
- Model route: custom `codex/gpt-5.6-sol`, variant `max`
- Gateway observed at investigation time: CLIProxyAPI `v7.2.143`, commit `4b5f1ea`, build date `2026-08-26T21:32:30Z`

This note intentionally excludes authorization headers, API keys, full prompts, and full model outputs.

## Directly observed failure signature

- The primary session completed six normal model turns and then produced two empty turns:
  - `2026-08-27 08:33:43 UTC`: `finish=unknown`, 0 input/output/reasoning/cache tokens, cost 0, duration 12.778 s.
  - `2026-08-27 08:36:50 UTC`: `finish=unknown`, 0 input/output/reasoning/cache tokens, cost 0, duration 21.698 s.
- Each empty turn contains only `step-start` and `step-finish(reason=unknown)` parts.
- The OpenCode runtime log contains no `stream error`, retry, abort, or cancellation for these turns. It logs `exiting loop` immediately afterward.
- The request context was not near the configured model limit. Tool output immediately before the failures was approximately 3 KB and 35 KB, respectively.
- Authenticated, non-generation `/models` probes returned HTTP 200 three times. Other model turns before and after the incident completed normally.
- A concurrent `providerID=openai` session logged `Token refresh failed: 401`. That was a separate OAuth-backed provider route, not the custom `codex` route used by the primary session.

## Client-side causal chain

The Responses/SSE stream reached OpenCode without a valid terminal event or finish reason. In `@ai-sdk/openai 3.0.53`, a stream that closes this way keeps the fallback finish state `other`. OpenCode maps that state to `unknown` and reports absent usage as zero tokens.

OpenCode `1.17.18` then treats every finish value other than `tool-calls`, including `unknown`, as terminal in `packages/opencode/src/session/prompt.ts`. It exits the agent loop rather than retrying or displaying an API error. This explains the user-visible symptom: the request appears to be sent, then nothing is returned.

Related OpenCode reports and proposed fix:

- https://github.com/anomalyco/opencode/issues/39968
- https://github.com/anomalyco/opencode/issues/41469
- https://github.com/anomalyco/opencode/issues/43622
- https://github.com/anomalyco/opencode/pull/43881

## Prompt-content correlation

- The primary session's copied starting context was about 7,038 characters and retained exploit-oriented security material: arbitrary file reads, unauthenticated endpoints, path traversal, SSRF, denial-of-service, PoC construction, and an encoded sensitive filesystem path.
- A newly created comparison session also failed while working on the security-team task. It first streamed roughly 50 seconds of reasoning about vulnerability packet preparation, SSRF feasibility, and PoC coordination, then stopped. This is inconsistent with a simple request-entry rejection and is more consistent with an output-side interruption or a transport failure during generation.
- Classification through `2026-08-27 09:08:32 UTC`:
  - Security-context turns: 233 total, 14 `unknown` finishes (6.01%).
  - Other-context turns: 83 total, 0 `unknown` finishes.
- The same model had 8,752 turns over the preceding five days with no `unknown` finish recorded.
- Failure rate did not materially improve when isolated from concurrency:
  - Concurrent turns: 253 total, 11 failures (4.35%).
  - Isolated turns: 63 total, 3 failures (4.76%).
- PoC/auth-oriented roles failed most often; surface-discovery work usually completed.

These observations support a content/semantic trigger, but do not establish a single bad keyword. Similar security language succeeded in some turns, including a neighboring session's compact summary.

## Official OpenAI cybersecurity-safeguard behavior

Official OpenAI documentation states that GPT-5.3-Codex and newer API models have additional automated cybersecurity safeguards. These systems monitor signals of potentially suspicious cybersecurity activity, and OpenAI explicitly notes that legitimate security research or defensive work may occasionally be flagged while the safeguards are calibrated.

The same documentation states:

- for approved API projects, `gpt-daybreak-blue-latest` resolves to `gpt-5.6-sol`;
- a safeguard action is surfaced with the error code `cyber_policy`;
- for Zero Data Retention organizations, request-level mitigations can occur and a streaming request may return `cyber_policy` in the middle of other stream events;
- API safeguards differ from safeguards in the Codex product surface.

Source: https://developers.openai.com/api/docs/guides/safety-checks/cybersecurity

This official behavior is a close match for the observed content correlation and mid-generation interruption. It does not by itself prove that these exact failed calls carried `cyber_policy`, because CLIProxyAPI/OpenCode did not preserve such an error and the route's organization/ZDR status is unknown. Gateway or upstream traces are still required for request-level attribution.

## Compaction and cache findings

- The primary session was not successfully compacted:
  - zero `type=compaction` parts;
  - zero assistant messages with `summary=true`.
- A neighboring session, `ses_fd599e7a6ffexhMxSnOHvfmsKP`, produced a 5,315-character security summary at about 08:24 UTC. That summary was then copied into the primary session.
- The summary preserved the exploit-oriented semantics, so compaction reduced length without removing the likely triggering content.
- The route uses `@ai-sdk/openai` with `setCacheKey: true`; OpenCode sets `promptCacheKey` to the session ID.
- Multiple distinct sessions and cache keys failed in security contexts. This makes a single poisoned session cache key substantially less likely.

Why a fresh session can appear to fix the issue: it often starts without the old exploit-oriented transcript, or regenerates the same intent with different wording. Once equivalent security context is copied or rebuilt, a new session can fail too.

## Gateway evidence

The configured API origin identified itself as `CLI Proxy API Server`. Response headers on an unauthenticated management probe identified the build as CLIProxyAPI `v7.2.143` (`4b5f1ea`).

CLIProxyAPI has documented failures in the same error family:

- Issue #4884: an upstream EOF during reasoning/generation could be converted into a synthetic `[DONE]`, hiding the interruption.
  - https://github.com/router-for-me/CLIProxyAPI/issues/4884
- PR #4711: a handler race could swallow upstream 502/429/auth errors and emit HTTP 200 plus `[DONE]`.
  - https://github.com/router-for-me/CLIProxyAPI/pull/4711
- PR #4580: production observations of HTTP 200, zero usage, and closure before `response.completed`, across multiple sessions and credentials.
  - https://github.com/router-for-me/CLIProxyAPI/pull/4580
- PR #4905: attempted handling for Responses streams that terminate without a terminal event; closed without merge.
  - https://github.com/router-for-me/CLIProxyAPI/pull/4905
- Issue #5028: recent regression/fix work around missing `[DONE]` and terminal semantics in the OpenAI-compatible path.
  - https://github.com/router-for-me/CLIProxyAPI/issues/5028

The current built-in Codex executor is intended to turn a missing `response.completed` into a stream error and the handler is intended to surface an error event. OpenCode received neither. Plausible explanations are:

1. the configured provider/plugin path bypassed that executor check;
2. a proxy or Cloudflare connection ended before the error frame reached the client;
3. another clean-close path still exists in the gateway or upstream.

Only gateway traces can distinguish these cases.

There is also a direct precedent for prompt fingerprints in this gateway ecosystem. CLIProxyAPI `v7.2.118` added Antigravity system-instruction keyword obfuscation after issue #4696 demonstrated strict A/B behavior where a specific system-prompt phrase changed an HTTP 200 response into a misleading quota HTTP 429:

- https://github.com/router-for-me/CLIProxyAPI/issues/4696

That fix is specific to the Antigravity provider. The investigated route uses `gpt-5.6-sol`, so it is evidence that content fingerprints can exist in an upstream route, not proof that this exact request used Antigravity or hit the same rule.

## Confidence-bounded conclusion

### Proven

- The failed Responses/SSE requests did not provide OpenCode with valid completion information.
- OpenCode converted the incomplete stream into `finish=unknown` and silently ended the loop.
- Failures were strongly concentrated in exploit/PoC/SSRF/path-traversal security contexts.
- A new session and a different cache key can also fail after comparable security context is introduced.
- Context overflow, ordinary credential expiry, concurrency, and a single poisoned session cache key do not explain the data.

### Strongly suspected

- An OpenAI API cybersecurity safeguard, or an equivalent upstream content-fingerprint mechanism, interrupts some exploit-oriented generations. Official documentation confirms that `gpt-5.6-sol` API traffic is subject to cybersecurity safeguards, that legitimate defensive work can be falsely flagged, and that a `cyber_policy` error can occur mid-stream.
- The CLIProxyAPI/plugin/proxy chain fails to preserve or expose the resulting upstream error, leaving OpenCode with a clean-looking EOF.
- Fresh sessions seem healthier because their prompt content or wording differs, not because the old session ID is permanently corrupted.

### Not yet proven

- Which exact layer closes the stream: model provider, provider adapter/plugin, CLIProxyAPI handler, or Cloudflare/downstream transport.
- Whether the exact upstream error for these failed requests was `cyber_policy`; the client-side evidence contains no preserved error code.
- Which rule, phrase combination, or generated passage triggers the interruption.
- That every failure is content-filter related; a stochastic stream/transport defect correlated with long security reasoning remains possible.

## Safe final diagnostic

Ask the gateway administrator to correlate one failed call by `X-CPA-TRACE-ID` or upstream request ID and record only:

- HTTP status at every hop;
- SSE event type sequence, especially whether `response.completed`, `response.failed`, or `event: error` appeared;
- which hop first observed EOF;
- provider/content-filter reason codes, if present;
- request and trace IDs.

Do not record Authorization values, API keys, the complete prompt, or complete generated output.

If gateway traces are unavailable, run a billed A/B test only with explicit approval: replay a minimized exploit-oriented passage versus a defensive rewrite with the same length and structure, each in fresh sessions and repeated enough times to distinguish deterministic filtering from random disconnects.

## Useful read-only checks

```bash
sqlite3 -readonly "$HOME/.local/share/opencode/opencode.db" \
  "SELECT id, json_extract(data,'$.finish'),
          json_extract(data,'$.tokens.input'),
          json_extract(data,'$.tokens.output'),
          json_extract(data,'$.tokens.reasoning'),
          json_extract(data,'$.cost')
   FROM message
   WHERE session_id='ses_fbda71076ffe1Krfu4sIgGnltY'
     AND json_extract(data,'$.role')='assistant'
   ORDER BY time_created;"

rg 'run=8aa37971|ses_fbda71076ffe1Krfu4sIgGnltY' \
  "$HOME/.local/share/opencode/log/opencode.log"
```
