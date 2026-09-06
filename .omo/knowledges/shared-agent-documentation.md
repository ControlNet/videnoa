# Shared agent documentation

- Track knowledges, notepads, plans, and drafts under `.omo/`. Other top-level `.omo` entries remain ignored because they contain local state, raw evidence, browser profiles, or benchmark artifacts.
- Before publishing documents, scan for credentials, personal absolute paths, institutional identifiers, and private deployment addresses. Preserve technical findings without machine-specific identifying details.
- The initial review covered all documents in these four directories. Personal paths and deployment identifiers were removed before staging; credential-pattern and case-insensitive identifier scans must be clean before publishing.
- Root ignore rules also exclude common credential and private-key files. Ignore rules do not remove previously tracked files or rewrite Git history.
