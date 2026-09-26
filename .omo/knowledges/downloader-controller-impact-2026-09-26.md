# Downloader isolation and Controller impact

Code-path assessment only; no production change or new runtime validation was
performed for this follow-up.

## Separate transfer paths

- Controller uploads the current local input through the Worker file API to
  `<task-id>/input.<extension>` in `scheduler/upload.rs`.
- It records the returned remote path and derives a sibling
  `output.<extension>` for the workflow in `finish_upload`.
- Controller retrieves the result through the file API at
  `<task-id>/output.<extension>` in `scheduler/download.rs`.
- Remote cleanup deletes the task workspace by task ID in
  `scheduler/cleanup_remote.rs`.
- These transfers do not invoke the core Downloader node, whose cache is under
  `std::env::temp_dir()/videnoa/downloads`.

Therefore, a scoped change to Downloader's private download-directory allocation
does not require changing Controller upload/download paths, publication,
persisted path evidence, or cleanup. This is conditional on preserving the
existing node output contract (`path: PortData::Path`) and Controller workflow
input/output contracts.

## Compatibility constraints for a future fix

1. Preserve the original sanitized leaf filename and extension in a unique
   directory. Renaming the leaf to a UUID would affect PathDivider-derived
   output names; changing only the parent still affects workflows that explicitly
   use parent_path or hardcode the former shared cache location.
2. Keep downloaded files alive while downstream nodes consume their returned
   paths, including FFmpeg reopening source media for audio/subtitle muxing.
   A node-local TempDir destroyed when execute returns would break consumers.
3. Do not redirect Controller-requested output paths into the download cache.
   Controller expects its configured task-workspace result path.
4. The Controller compatibility check validates Path inputs named input/output;
   it does not prohibit Downloader inside an otherwise eligible workflow. Such
   a workflow is affected by both the existing collision bug and its fix.
5. Controller's task-workspace deletion does not collect Downloader cache files.
   Per-execution isolation increases the importance of an explicit file-lifetime
   and cleanup policy; cleanup must not delete unrelated files or outputs that a
   workflow intentionally placed alongside a downloaded input.

If the user instead means the existing bug's impact, ordinary Controller file
transfer does not share the vulnerable basename cache. A dispatched workflow
containing Downloader can still produce incorrect content after a collision.
