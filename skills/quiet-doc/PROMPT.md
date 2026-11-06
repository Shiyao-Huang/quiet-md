# Quiet CLI agent prompt

You operate a local-first Quiet.md document library.

## Quick route

- Read local document: `quiet docs +get DOCUMENT_ID`
- List/search local documents: `quiet docs +list` or `quiet docs +search QUERY`
- Create: `quiet docs +create --title TITLE --content CONTENT --content-format markdown|xml`
- Replace: `quiet docs +update DOCUMENT_ID --content CONTENT --content-format markdown|xml`
- Append: `quiet docs +append DOCUMENT_ID --content CONTENT --content-format markdown|xml`
- Import a local file: `quiet docs +import FILE`
- Search current user's Feishu materials: `quiet +search --query QUERY --mine`
- Fetch a Feishu document: `quiet +fetch --doc URL_OR_TOKEN --doc-format markdown`
- Import fetched Feishu content into a scratch library: `quiet --library /tmp/quiet-lib +import --doc URL_OR_TOKEN --doc-format markdown`

## Write protocol

1. Choose the narrowest local command. Do not use a full replacement when append or metadata is sufficient.
2. Use `--dry-run` for a write when the input or destination is uncertain.
3. Use `--yes` for delete, restore, purge, empty, and other high-risk commands.
4. Keep tests and imports in an explicitly isolated `--library` path; never use a production library as a fixture.
5. After a write, fetch or list the affected local document and run `quiet library doctor` when the library structure changed.

Quiet is local. It does not provide Feishu OAuth or cloud permissions. The root `+search`, `+fetch`, and `+import` shortcuts delegate selected remote material operations to the installed `lark-cli`.
