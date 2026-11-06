# Quiet document update and append

Use this reference for local document edits. Unlike cloud block APIs, Quiet CLI writes the local document model atomically; it does not expose remote block IDs.

## Observe first

```bash
quiet docs +get DOCUMENT_ID
quiet docs +export DOCUMENT_ID --as markdown
```

Choose the smallest operation that satisfies the request. Keep the current title, unsupported blocks, assets, and DocxXML structure unless the user explicitly asks to replace them.

## Replace the document body

```bash
quiet docs +update DOCUMENT_ID \
  --content "# New body\n\nUpdated text" \
  --content-format markdown \
  --dry-run

quiet docs +update DOCUMENT_ID \
  --content "# New body\n\nUpdated text" \
  --content-format markdown
```

Use XML when the document contains structures not represented by Markdown. Do not use a full replacement merely to append one paragraph.

## Append content

```bash
quiet docs +append DOCUMENT_ID \
  --content "\n\n## New section\n\n追加内容" \
  --content-format markdown \
  --dry-run
```

After every write, re-read the affected document:

```bash
quiet docs +get DOCUMENT_ID
```

If a write changes the document tree or imports assets, run `quiet library doctor` against the same `--library`.
