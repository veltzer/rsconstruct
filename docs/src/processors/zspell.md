# Zspell Processor

## Purpose

Checks documentation files for spelling errors using Hunspell-compatible
dictionaries (via the `zspell` crate, pure Rust).

## How It Works

Discovers files matching the configured extensions, extracts words from
markdown content (stripping code blocks, inline code, URLs, and HTML tags),
and checks each word against the system Hunspell dictionary and a custom
words file (if it exists). Fails with a list of misspelled words on error.

Dictionaries are read from `/usr/share/hunspell/`.

This processor supports batch mode when `auto_add_words` is enabled, collecting
all misspelled words across files and writing them to the words file at the end.

## Source Files

- Input: `**/*{extensions}` (default: `**/*.md`)
- Output: none (checker)

## Custom Words File

The processor loads custom words from the file specified by `words_file`
(default: `.zspell-words`) if the file exists. Format: one word per line,
`#` comments supported, blank lines ignored.

The words file is also auto-detected as an input via `auto_inputs`, so changes
to it invalidate all zspell products. To disable words file detection, set
`auto_inputs = []`.

## Configuration

```toml
[processor.zspell]
extensions = [".md"]                  # File extensions to check (default: [".md"])
language = "en_US"                    # Hunspell dictionary language (default: "en_US")
words_file = ".zspell-words"          # Path to custom words file (default: ".zspell-words")
auto_add_words = false                # Auto-add misspelled words to words_file (default: false)
auto_inputs = [".zspell-words"]       # Auto-detected config files (default: [".zspell-words"])
extra_inputs = []                     # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extensions` | string[] | `[".md"]` | File extensions to discover |
| `language` | string | `"en_US"` | Hunspell dictionary language (requires system package) |
| `words_file` | string | `".zspell-words"` | Path to custom words file (relative to project root) |
| `auto_add_words` | bool | `false` | Auto-add misspelled words to words_file instead of failing (also available as `--auto-add-words` CLI flag) |
| `auto_inputs` | string[] | `[".zspell-words"]` | Config files auto-detected as inputs |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
