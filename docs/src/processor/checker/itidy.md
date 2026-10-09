# Itidy Processor

## Purpose

Checks HTML files the way `tidy -errors` does: structure, missing and misplaced tags, unknown elements, attributes and attribute values, entities, and the repairs tidy would make. Native (in-process; no `tidy` binary). The Rust alternative to the [tidy](tidy.md) processor: a port of HTML Tidy 5.8.0's lexer, parser, attribute checks and clean-up passes, so it reports what tidy reports, at the same line and column, with the same warning and error totals. The pretty-printer is not ported: itidy never writes HTML.

## How It Works

Each file is tokenized and parsed with tidy's own algorithm, element by element, inferring the start and end tags the markup leaves out, moving misplaced content where tidy moves it and reporting every repair. The attribute checks (which attributes an element allows in the document's HTML version, their values, `id` and `name` uniqueness, URIs) and the clean-up passes that follow parsing (style elements in the body, the meta charset, the doctype, HTML5-removed elements, proprietary elements and attributes) run as well. Problems are reported in tidy's format, with the file in front:

```text
docs/page.html: line 3 column 7 - Error: <foo> is not recognized!
docs/page.html: line 3 column 7 - Warning: discarding unexpected <foo>
docs/page.html: line 12 column 5 - Warning: <img> lacks "alt" attribute
docs/page.html: line 20 column 1 - Warning: trimming empty <p>
docs/page.html: line 31 column 17 - Warning: <div> proprietary attribute "ng-model"
```

The verdict is tidy's: a file with warnings or errors fails (tidy exits 1 or 2), a clean file passes. The totals follow tidy's counting, including the fact that tidy stops printing reports after `show-errors` (6) errors while still counting them. Only the report lines are printed; tidy's trailing dialogue ("Tidy found 2 warnings and 1 error!", "This document has errors that must be fixed...") carries nothing a build needs and is left out, as are the `Info:` lines `-q` hides.

### Options

tidy's options drive the checks exactly as they drive `tidy`. `config_file` names a tidy configuration file (`name: value` lines, `#` comments, continuation lines), read like `tidy -config FILE`. `options` lists further settings as `"name: value"` strings, applied after the file in order, like `--name value` arguments:

```toml
[processor.checker.itidy]
src_dirs = ["docs"]
options = ["custom-tags: blocklevel", "drop-empty-elements: no"]
```

Every option of tidy 5.8.0 is accepted and its value validated, so an existing tidy configuration parses unchanged. Options that only shape tidy's pretty-printed output (`indent`, `wrap`, `output-file`, ...) are ignored. A few are refused by name because itidy does not reproduce their effect: `clean`, `gdoc`, `bare`, `word-2000` and `logical-emphasis` set to `yes`, `input-xml: yes`, `accessibility-check` above 0, `mute`, and any input or output encoding other than `utf8`. An unknown option is a build error, where `tidy` prints a notice and carries on.

Input is UTF-8 (a UTF-8 byte order mark is accepted; a UTF-16 one is an error naming the file). Tabs count as `tab-size` (8) columns, as in tidy, so column numbers agree.

### Fidelity

Checked against `tidy -errors -q` 5.8.0 over the fleet's 599 tracked HTML files and 6000 mutated copies of them (dropped and swapped tags, unquoted and duplicated attributes, bad entities, truncated files, control characters, invalid UTF-8, deprecated elements, misplaced list items and table cells): identical report lines, in the same order, for every file, under the default options and under the fleet's option sets (`custom-tags`, `drop-empty-elements`, `new-blocklevel-tags`, `drop-empty-paras`, `warn-proprietary-attributes`). The quirks this required are documented in the source: tidy treats an empty attribute value as absent, does not count an ESC character as a column, and reads a byte past the end of an emptied text node to decide whether it was blank.

## Source Files

- Input: `.html` and `.htm` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.itidy]
src_dirs = ["docs"]
config_file = "support/tidy.conf"
options = ["warn-proprietary-attributes: no"]
dep_inputs = ["support/tidy.conf"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `""` | A tidy configuration file read like `tidy -config`; relative to the project root. Empty: none |
| `options` | string[] | `[]` | Further tidy options as `"name: value"` strings, applied after the config file, like `--name value` arguments |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".html", ".htm"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (put the config file here) |

Switching a repo from tidy: `[processor.checker.tidy]` → `[processor.checker.itidy]`; `args = ["-config", "x.conf"]` → `config_file = "x.conf"`; `args = ["--name", "value"]` → `options = ["name: value"]`; `-errors`, `-q` and `-utf8` are the behaviour already and are dropped.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
