# Controller Add Task Completion

Implemented on dev after the committed frontend redesign (79a6547).

- Referenced Videnoa `web/src/components/shared/PathAutocomplete.tsx` and
  `WorkflowPathPicker.tsx`: 300ms debounce, filename-prefix matching, directories
  first, directory selection continues browsing, file selection closes.
- Controller retains its own CSS tokens, ApiClient, session handling, and Worker
  registry. No Radix/Tailwind/i18n dependency or GPU core import was introduced.
- Authenticated GET `/api/task-path-suggestions?kind=input|output&prefix=...`
  uses descriptor-backed no-follow directory access on a blocking task. It
  excludes private data/temp, symlinks, traversal and non-regular entries.
  Input suggests files/directories; output suggests directories only. Selection
  returns absolute process-visible paths. Browsing neither opens file contents
  nor creates media files. Intake retains independent full validation.
- Each request scans up to 4096 directory entries and returns up to 100 matches;
  the truncated flag explicitly reports incomplete results. No HTTP caching.
- Workflow completion reuses GET `/api/workers`, filters enabled workers, merges
  compatible cached workflow/preset names, and deduplicates by name. Offline
  enabled workers retain their last advertised names. Manual typing stays valid.
- Editable ARIA comboboxes support arrows, Enter selection, Escape dismissal,
  IME composition, focus preservation, request cancellation and stale responses.

Validation passed:

```bash
cargo +1.83.0 fmt --all -- --check
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --all-targets
npm --prefix controller-web run lint
npm --prefix controller-web test
npm --prefix controller-web run build
npm --prefix controller-web run test:e2e -- task-completion.spec.ts task-creation.spec.ts
bash scripts/tests/controller_docs_test.sh
```

Frontend: 137 unit tests; 2 new HTTP completion E2E tests and 4 existing creation
E2E tests passed. E2E uses a real insecure browser origin and synthetic test-only
directory/Worker responses. Backend tests use temporary directories and the
existing low-cost test PHC plus reusable real session; production auth unchanged.
The full backend suite passed with its existing ignored Argon2 stress test.
Screenshots inspected: `.omo/evidence/controller-task-completion-1280.png` and
`controller-task-completion-390.png`. Axe checks passed on the open modal.

No Docker build or live container replacement was requested or performed.
