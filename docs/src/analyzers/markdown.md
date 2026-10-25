# markdown

Scans Markdown source files for image and link references (`![alt](path)`, `[text](path)`) and adds referenced local files as dependencies.

**Builtin**: Yes.

**Auto-detects**: Projects with `.md` files.

## Features

- Extracts `![alt](path)` image references and `[text](path)` link references
- Resolves paths relative to the source file's directory
- Skips URLs (`http://`, `https://`, `ftp://`), data URIs, and anchor-only links
- Strips title text and anchor fragments from paths

This ensures that when an image or linked file changes, any Markdown product that references it is rebuilt.

## Configuration

```toml
[analyzer.markdown]
# currently no tunables
```
