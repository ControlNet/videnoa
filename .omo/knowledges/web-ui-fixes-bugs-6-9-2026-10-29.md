# Worker web UI fixes for review findings 6-9 (2026-10-29)

These fix findings 6-9 of `post-v0.1.10-review-2026-10-29.md`.

## What changed

- **6, saved-workflow delete:** `PresetGallery.tsx` opens a confirmation
  dialog that names the workflow. Only the destructive button sends DELETE,
  using the filename of the row that was clicked. Job-history delete on the
  Jobs page still works with one click on purpose: it removes a history
  record, not a user file.
- **7, Jobs page errors:** cancel, retry and history-delete failures show
  `"<action>: <server message>"` toasts through `notifyActionFailure` in
  `JobsPage.tsx`. A 401 is skipped there.
- **8, editor previews:** `workflow-store.submittedJobId` records the job
  returned by the editor's own submit paths (`EditorToolbar.handleRun` and
  `RunFromEditorDialog.handleSubmit`). `EditorPage` tracks only that job while
  it is running or queued.
  - `loadWorkflow` and `clear` set the association to null and call
    `useJobStore.getState().unsubscribeFromJob()`, which also clears
    `runtimePreviewsByNodeId`.
  - The association is kept in memory only, so a page reload loses it.
- **9, preview retention:** `deleteJobHistory`, which cancel also uses, drops
  `runtimePreviewsByJobId[jobId]` after the server confirms the delete.
  Socket close and list refresh keep the entry, because the Print output
  panel for finished jobs reads it.

## Reusable lessons

- **Radix Escape handling:** Radix `DismissableLayer` closes the top-most
  layer from a capture-phase `keydown` listener on `document`. It calls
  `preventDefault()` but not `stopPropagation()`. A window-level Escape
  shortcut must therefore check `e.defaultPrevented`. Otherwise it also
  closes the modal underneath, as the `EditorPage` shortcut did.
- **Radix outside clicks:** with nested modal dialogs, only the top layer
  reacts to an outside pointerdown, so the gallery stays open.
- **Dialogs inside clickable elements:** render a confirmation `<Dialog>`
  outside any clickable element such as a `Card` with `onClick`. Radix portals
  the DOM, but React synthetic events still bubble through the component tree.
- **401 responses:** `authenticatedFetch` dispatches `videnoa-auth-expired`,
  and `AuthGate` then shows the sign-in screen. UI error toasts should skip
  `ApiError` with status 401 so the notice does not appear twice.
- **Testing modal dialogs:** while a modal confirmation is open, the dialog
  underneath is `aria-hidden`. `getByRole` cannot find it, even with
  `hidden: true` and a name, so use `getByText`.
