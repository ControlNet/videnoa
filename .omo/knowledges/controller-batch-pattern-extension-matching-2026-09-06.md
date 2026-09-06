# Batch wildcard extension matching

- `paths/batch.rs` matches filenames with glob patterns without restricting matches to video extensions. Chinese characters and spaces are ordinary filename characters. Matching is case-sensitive on Linux.
- A pattern ending in `S01E01.*` requires a literal dot after the episode number. It can include sidecars and existing generated files, not only the original video.
- With `naming_mode: insert_extension` and `middle_extension: AI`, `S01E01.mkv` maps to `S01E01.AI.mkv`; `S01E01.zh.ass` maps to `S01E01.zh.AI.ass`. Existing outputs reject the whole synchronous batch during preview.
- Recommend an exact known video filename/extension when selecting one episode. A successful request schema check does not prove the Controller/container can access the path or that a worker has the requested workflow.
- The user's supplied `/downloads/Bangumi` season directory was not present in this agent environment. No live request or task was submitted.

## Brace alternatives implemented

- Both batch endpoints and the UI now accept comma-separated brace alternatives, e.g. `魔女之旅 S01E01.{mkv,mp4,avi,mov,webm}`.
- `paths/batch_pattern.rs` compiles bounded alternatives per path component into existing glob patterns. Components match any alternative during the existing single traversal; duplicate alternatives do not duplicate files, and scan/file/depth limits remain shared.
- Multiple groups, directory alternatives, `*`, `?`, character classes and standalone recursive `**` compose. Braces inside character classes remain literal; `[{]` and `[}]` match braces.
- Groups require two non-empty alternatives, do not nest or cross separators, and are limited to 64 expanded patterns per component. Numeric ranges and recursive `**` inside alternatives are rejected. Linux matching remains case-sensitive.
- Five synthetic HTTP regressions cover Chinese names, video-vs-sidecar selection, duplicate options, combined directory/recursive/class patterns, malformed patterns, preview zero-creation on conflicts, and the shared 500-file limit. Two parser tests cover literal braces, character classes, Cartesian expansion limits and traversal rejection.
- Red-first: all four initial HTTP regressions failed before implementation. After implementation, 29 Task API tests and one concurrency test passed; nine path capability tests and two parser tests passed. Final wildcard target includes the fifth HTTP regression for 501 files across extensions.
- Verification commands: `cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency`; `cargo +1.83.0 test --locked -p videnoa-controller --lib batch_pattern`; `cargo +1.83.0 test --locked -p videnoa-controller --test path_capabilities`; `cargo +1.83.0 test --locked -p videnoa-controller --test task_api batch_wildcards`; strict all-target/all-feature Clippy; workspace fmt and direct rustfmt on batch modules; `bash scripts/tests/controller_docs_test.sh`; frontend `npm run lint` and `npm run build`.
- Tests use explicitly synthetic media only. No real videos were processed and no service was deployed.
