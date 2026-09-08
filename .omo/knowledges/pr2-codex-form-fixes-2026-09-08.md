# PR #2 confirmed Codex form fixes

## Settings draft preservation

- Stop remounting the editor on every settings version. Only a successful explicit save establishes a fresh editor and baseline.
- On an authoritative version change, compare each local field against its prior baseline. Preserve edited fields, refresh untouched fields, and use the new version for the next save. Versionless scheduler deltas update scheduler status without reseeding fields.
- Notify the operator that local edits were retained and that saving applies those values over current Controller values. This includes conflicts where the same field was edited remotely; saving is explicit, never automatic.
- Keep an existing editor mounted when a background settings/readiness refresh fails. Disable save/pause while loading or in the failed-refresh state, and reject direct save callbacks until settings are available again. Retry can recover without dropping the draft.
- Regression covers unsaved server/auth/retry fields, same-field remote changes, untouched scheduler/timeout refreshes, input identity, current-version saves, successful-save baseline reset, and failed-refresh recovery. Existing browser conflict coverage now requires unsaved uploads to survive instead of retyping the value after the conflict.

## HTTP-to-Iroh password transition

- Reset `clearPassword` when changing transport using a functional state update.
- After selecting removal on HTTP and switching to Iroh, the password field is enabled. Blank keeps the saved password (omits the update field), while entering a replacement sends that replacement. Switching back to HTTP shows an unchecked removal option.
- Existing explicit HTTP removal behavior remains covered.

## Verification

Four new regression cases failed before the fixes (two settings cases, two keep/replace password variants). All passed afterward. All fixtures and replacement credentials are synthetic test-only data; no actual Worker or credentials are used.

```bash
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
npm --prefix controller-web run test:e2e
git diff --check
```

Expected: 167 unit tests pass; lint and TypeScript/Vite build exit zero; browser suite passes, including retained edits after a version conflict. Candidate CI is rerun on push; prior CI results do not certify this new commit. Only the two confirmed Codex findings are addressed; the lower-priority Worker WebUI reload-error finding is outside this patch.

Final local results: 167 unit tests passed; lint/build passed; 67/67 browser tests passed. The first browser run passed 66/67 with a blank-page timeout in an unchanged task URL test while an additional build was running against its served assets. The affected suite and operations tests then passed 10/10, and the final full browser run passed 67/67 without concurrent builds. No timeout or task behavior was changed.
