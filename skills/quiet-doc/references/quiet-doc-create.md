# Quiet document create and import

Use this reference for `quiet docs +create`, `quiet docs +import`, and the root `quiet +import` shortcut.

## Create a local document

```bash
quiet docs +create \
  --title "TITLE" \
  --content "# Heading\n\nBody" \
  --content-format markdown
```

For full Quiet block fidelity, use DocxXML:

```bash
quiet docs +create \
  --title "TITLE" \
  --file ./document.xml \
  --content-format xml
```

Use `--parent ID` only after checking the parent exists. Use `--parent root` for a root document. Use `--dry-run` when the title, format, or parent is not yet confirmed.

## Import a local file

```bash
quiet --library /tmp/quiet-lib docs +import ./draft.md --dry-run
quiet --library /tmp/quiet-lib docs +import ./draft.md
```

The import must use an isolated library for tests and smoke runs. Do not overwrite or clean a user's default library as part of a test.

## Import selected Feishu material

```bash
quiet --library /tmp/quiet-lib +import \
  --doc URL_OR_TOKEN \
  --doc-format markdown \
  --dry-run

quiet --library /tmp/quiet-lib +import \
  --doc URL_OR_TOKEN \
  --doc-format markdown
```

The root shortcut fetches the remote document through `lark-cli`, converts Markdown or DocxXML into the Quiet document model, and writes only to the selected local library. Verify the result with:

```bash
quiet --library /tmp/quiet-lib docs +list
quiet --library /tmp/quiet-lib library doctor
```
