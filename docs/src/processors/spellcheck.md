# Spellcheck Processor

## Purpose

Checks documentation files for spelling errors using Hunspell-compatible
dictionaries (via the `zspell` crate, pure Rust).

## How It Works

Discovers files matching the configured extensions, extracts words from
markdown content (stripping code blocks, inline code, URLs, and HTML tags),
and checks each word against the system Hunspell dictionary and an optional
custom words file. Creates a stub file on success; fails with a list of
misspelled words on error.

Dictionaries are read from `/usr/share/hunspell/`.

## Source Files

- Input: `**/*{extensions}` (default: `**/*.md`)
- Output: `out/spellcheck/{flat_name}.spellcheck`

## Custom Words File

When `use_words_file = true`, the processor loads custom words from the file
specified by `words_file` (default: `.spellcheck-words`). The file must exist.
Format: one word per line, `#` comments supported, blank lines ignored.

The custom words file is loaded once at processor initialization and is not
part of the cache key. Adding a word does not cause all files to rebuild.

## Configuration

```toml
[processor.spellcheck]
extensions = [".md"]                  # File extensions to check (default: [".md"])
language = "en_US"                    # Hunspell dictionary language (default: "en_US")
words_file = ".spellcheck-words"      # Path to custom words file (default: ".spellcheck-words")
use_words_file = false                # Enable custom words file (default: false)
extra_inputs = []                     # Additional files that trigger rebuilds when changed
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extensions` | string[] | `[".md"]` | File extensions to discover |
| `language` | string | `"en_US"` | Hunspell dictionary language (requires system package) |
| `words_file` | string | `".spellcheck-words"` | Path to custom words file (relative to project root) |
| `use_words_file` | bool | `false` | Load the custom words file (file must exist when enabled) |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
