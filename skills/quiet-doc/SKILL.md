---
name: quiet-doc
description: "Quiet.md local document authoring and Feishu-compatible material bridge. Use for local document read/create/update/append/import/export/search, or for the root +search/+fetch/+import shortcuts."
metadata:
  requires:
    bins: ["quiet"]
  prompt: "PROMPT.md"
---

# Quiet document skill

Read [`PROMPT.md`](PROMPT.md) first. Then read only the reference for the operation being performed:

- **Create or import**: [`references/quiet-doc-create.md`](references/quiet-doc-create.md)
- **Read**: use `quiet docs +get`, `quiet docs +list`, or `quiet docs +search`; remote material uses `quiet +fetch`
- **Update or append**: [`references/quiet-doc-update.md`](references/quiet-doc-update.md)

## Routing

### Local Quiet library

Use `quiet docs ...` for documents already in the local Quiet.md library. The local library is the source of truth for local writes; do not edit its XML or `index.json` directly.

### Feishu-compatible material bridge

Use the root Quiet shortcuts for the selected remote operations:

```bash
quiet +search --query "" --mine
quiet +fetch --doc URL_OR_TOKEN --doc-format markdown
quiet --library /tmp/quiet-lib +import --doc URL_OR_TOKEN --doc-format markdown
```

The bridge delegates to `lark-cli`, defaults to the user identity, and does not implement OAuth. `data.result` contains the remote response; local imports write only to the selected Quiet library.

## Contract and safety

- JSON is the default output; stdout contains successful responses and stderr contains errors.
- Use `--format pretty|table|ndjson|csv` only when a human or downstream consumer needs that representation.
- Use `--dry-run` before uncertain writes.
- Do not use the default library in tests or import smoke runs; pass `--library` to an isolated temporary directory.
- After local writes, verify with `quiet docs +get` or `quiet docs +list`; after tree/import changes, run `quiet library doctor`.
- Preserve DocxXML when Markdown cannot represent the document's structure. Do not silently flatten unsupported blocks, assets, or references.

## Out of scope

This skill does not perform Feishu OAuth, cloud permission administration, messaging, calendar, mail, Base, sheet, or wiki operations. Keep those operations in `lark-cli` skills.
