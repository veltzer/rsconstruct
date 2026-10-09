# Isvglint Processor

## Purpose

Lints SVG files with [svglint](https://github.com/simple-icons/svglint)'s rules, in-process (no Node, no `svglint` binary). The Rust alternative to the [svglint](svglint.md) processor: a port of svglint 4.2.1's three built-in rules and of the machinery they run on, so a file gets the same findings, with the same messages, as `svglint` would give it under the same rules.

## How It Works

Each file is parsed the way svglint parses it (htmlparser2 in XML mode, leniently: an unclosed tag closes at the end of the file, a repeated attribute keeps its first value, entities stay as written), then every configured rule runs over the result:

- `valid` (on by default) checks that the file is well-formed XML, with fast-xml-parser's validator and its messages.
- `elm` checks which elements appear, by CSS selector: present, absent, an exact count or a range.
- `attr` checks the attributes of the elements a CSS selector picks: required, forbidden, exact values, one of several values, a regular expression, attribute order and a whitelist.

svglint's `custom` rules are JavaScript functions and have no counterpart. Findings are printed one per line, the rule's name as svglint shows it (`valid`, `elm`, `attr-1` for the first `attr` table, ...), with the element and its position when there is one:

```text
img/icon.svg:1:1: attr-1: Wrong ordering of attributes, found "xmlns, viewBox, role", expected "role, viewBox, xmlns" (<svg>)
img/icon.svg:3:3: attr-2: Expected attribute 'fill' to be one of ["red","blue"], was "green" (<path>)
img/icon.svg:4:3: elm: Element disallowed (<rect>)
img/icon.svg: elm: Found 0 elements for 'svg > title', expected 1
img/icon.svg: valid: Attribute 'fill' is repeated.
```

Any finding fails the file, as svglint's exit status does. Positions are 1-based line and column of the element's start tag (svglint's own output prints them 0-based and miscounts lines after a leading newline; the elements are the same).

### Rules in TOML

svglint reads its rules from a JavaScript file; isvglint takes the same rules as TOML in the stanza:

```toml
[processor.checker.isvglint]
src_dirs = ["icons"]

[processor.checker.isvglint.elm]
"svg" = 1               # exactly one
"svg > title" = true    # at least one
"svg > path" = [1, 3]   # between one and three
"*" = false             # nothing else (an element another rule allows is fine)

[[processor.checker.isvglint.attr]]
"rule::selector" = "svg"
"rule::whitelist" = true          # no attributes beyond those named here
"rule::order" = true              # alphabetical; or a list giving the order
xmlns = "http://www.w3.org/2000/svg"
role = "img"
viewBox = { regex = "^0 0 \\d+ \\d+$" }
"width?" = true                   # optional: checked when present

[[processor.checker.isvglint.attr]]
"rule::selector" = "svg > path"
d = { regex = "^m[-mzlhvcsqtae\\d,. ]+$", flags = "i" }
fill = ["red", "blue"]
onclick = false
```

An `attr` value is `true` (must exist), `false` (must not), a string (that exact value), an array of strings (one of them) or `{ regex = "...", flags = "..." }` (a JavaScript-style regular expression; `flags` take `i`, `m`, `s`). A key ending in `?` is optional. Selectors follow css-select's XML-mode semantics: case-sensitive names, `#id`, `.class`, all attribute operators including `!=` and the `i` flag, the four combinators, selector lists, `:not`, `:is`, `:has` (with a leading combinator), the `-child` and `-of-type` families with `An+B`, `:empty`, `:contains`, `:icontains`. A selector using anything else (pseudo-elements, `:root`, `:scope`, namespaces) is a configuration error, as is a rule value of the wrong shape; svglint would warn and go on.

With no `elm` or `attr` configured, only `valid` runs, exactly like `svglint` without a `.svglintrc.js`.

### Fidelity

Checked against svglint 4.2.1 over the fleet's 5776 tracked SVG files and 3000 mutated copies of them (dropped and renamed closing tags, duplicate, unquoted, valueless and boolean attributes, stray ampersands, extra roots, truncation, misplaced XML declarations, CDATA and comment edge cases, byte order marks, control characters), under the default rules and two rule sets exercising every selector feature and attribute check above: the same findings, file for file. The lenient parser is a step-for-step port of htmlparser2 3.10.1, including how it chains one node's start position from the previous node's end; the validator is a port of fast-xml-parser 5.10.1, including that a self-closing root is never "multiple roots" and that an XML declaration after a tag is read as an instruction named `?xml` and passes.

## Source Files

- Input: `.svg` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `valid` | bool | `true` | The `valid` rule: every file must be well-formed XML |
| `elm` | table | `{}` | The `elm` rule: CSS selector = `true`, `false`, a count or `[min, max]` |
| `attr` | array of tables | `[]` | The `attr` rule: one table per `"rule::selector"`, naming the attributes and their values |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".svg"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Switching a repo from svglint: `[processor.checker.svglint]` → `[processor.checker.isvglint]`; the rules of `.svglintrc.js` become the `elm` table and `attr` tables above (a `/regex/i` literal is `{ regex = "regex", flags = "i" }`); `custom` rules have to stay with svglint.

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
