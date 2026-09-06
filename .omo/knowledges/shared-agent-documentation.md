# Shared agent documentation

- Track knowledges, notepads, plans, and drafts under `.omo/`. Other top-level `.omo` entries remain ignored because they contain local state, raw evidence, browser profiles, or benchmark artifacts.
- Before publishing documents, scan for credentials, personal absolute paths, institutional identifiers, and private deployment addresses. Preserve technical findings without machine-specific identifying details.
- The initial review covered all documents in these four directories. Personal paths and deployment identifiers were removed before staging; credential-pattern and case-insensitive identifier scans must be clean before publishing.
- Root ignore rules also exclude common credential and private-key files. Ignore rules do not remove previously tracked files or rewrite Git history.

## History cleanup (2026-09-06)

- Rewrote the affected development history to remove personal checkout paths from three historical blobs in two Controller evidence records.
- Scanned 2,091 reachable historical file blobs across remote branches and tags; the requested private identifiers had no remaining matches.
- Verified the development tip tree was identical before and after rewriting, and checked Git object integrity. Only the development branch changed; the main branch and release tags retained their original object IDs.
- Published using an explicit expected remote commit lease and synchronized the active checkout. Existing clones must adopt the rewritten history before pushing to avoid reintroducing removed records.
- Rewriting reachable history does not purge GitHub cached commit views, fork references, or other clones. GitHub Support controls server-side cache and object removal.

## Local verification artifacts

- Removed the previously tracked 24 evidence files and one benchmark file from the Git index while retaining their local contents. Both directories remain ignored.
- Keep durable conclusions and reproducible verification commands in knowledge documents; treat raw evidence and benchmark outputs as local artifacts.
- This index cleanup does not remove files from earlier commits.
