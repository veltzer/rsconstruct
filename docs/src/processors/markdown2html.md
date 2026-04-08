# Markdown2html Processor

## Purpose

Converts Markdown files to HTML using the [markdown](https://daringfireball.net/projects/markdown/) Perl script.

## How It Works

Discovers `.md` files in the project and runs `markdown` on each file,
producing an HTML output file.

## Source Files

- Input: `**/*.md`
- Output: `out/markdown2html/{relative_path}.html`

## Configuration

```toml
[processor.markdown2html]
markdown_bin = "markdown"              # The markdown command to run
args = []                              # Additional arguments to pass to markdown
output_dir = "out/markdown2html"       # Output directory
extra_inputs = []                      # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `markdown_bin` | string | `"markdown"` | The markdown executable to run |
| `args` | string[] | `[]` | Extra arguments passed to markdown |
| `output_dir` | string | `"out/markdown2html"` | Output directory for HTML files |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

Each input file is processed individually, producing its own output file.
