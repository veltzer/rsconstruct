# Ixmllint Processor

## Purpose

Checks that XML files are well-formed and, with `schema`, that they validate against an XSD schema. Native (in-process; no libxml2, no external tools). The Rust alternative to the [xmllint](xmllint.md) processor: it reports what `xmllint --noout` and `xmllint --noout --schema x.xsd` report, at the same line, with the same verdict.

## How It Works

Each file is decoded as its XML declaration says (UTF-8 by default; UTF-16, ISO-8859-1 and US-ASCII are understood, another declared encoding is an error), tokenized, and checked for everything xmllint checks: tag matching, duplicate attributes, entity and character references against the internal DTD subset, namespace declarations and prefixes, and the document's shape. Problems are reported in xmllint's format, one per line:

```text
test.xml:3: parser error : Opening and ending tag mismatch: b line 2 and a
test.xml:6: parser error : Attribute b redefined
test.xml:1: namespace error : Namespace prefix x on b is not defined
test.xml:7: Schemas validity error : Element '{urn:keynote}slidex': This element is not expected. Expected is ( {urn:keynote}slide ).
```

The verdict is xmllint's. A `parser error` (the file is not XML) or a `Schemas validity error` fails the file. A `namespace error`, `namespace warning` or `parser warning` is printed and the file passes, because `xmllint` exits 0 on those; `strict = true` makes them fail too.

Where xmllint reports a problem is reproduced as well: a duplicate attribute, an undeclared prefix or a schema violation is reported on the line where the element's start tag ends (libxml2 checks the tag once it is complete and stamps the node with that line), an unfinished document on its last line, and only when nothing else went wrong first.

### Schema validation

`schema` names an XSD file, relative to the project root, like `--schema`. Every file must validate against it; the schema is read once per build. The schema file is not tracked automatically: list it in `dep_inputs` so a change to it re-checks every file.

The validator implements the schema constructs in use: global and local elements, named and anonymous complex types with `sequence`, `choice` and `all` content models (with `minOccurs`/`maxOccurs`), `any`, mixed and simple content, attributes with `use` and `fixed`, `anyAttribute`, simple types restricted by the standard facets (`enumeration`, `pattern`, lengths, bounds, digits, `whiteSpace`), `list` and `union`, `extension` of simple and complex types, `include`, and the built-in types from `xs:string` to `xs:dateTime`. A schema using anything else (`group`, `attributeGroup`, `import`, `redefine`, `key`/`unique`, substitution groups, `abstract`, complex-type `restriction`, `xsi:type` in a document) is a build failure naming the construct and its line: a schema that is only half enforced would pass files xmllint rejects. Keep such a file on the [xmllint](xmllint.md) processor, or extend `src/engines/xmllint/xsd.rs`.

Messages follow xmllint's wording (`This element is not expected. Expected is ( ... ).`, `The attribute 'a' is required but missing.`, `'x' is not a valid value of the atomic type 'xs:int'.`, `[facet 'enumeration'] The value 'v' is not an element of the set {'a', 'b'}.`).

### Fidelity

Checked against xmllint (libxml2 2.15) over the fleet's 6256 tracked XML and SVG files plus 58 hand-made edge cases: every file gets the same verdict, every problem is reported at the same line with the same class, and the messages read the same. Schema validation was checked over the one fleet schema (java-keynote) with its presentations and eight mutations of them: identical reports, line for line.

Two differences remain. After a lexical error (a bad comment, an invalid character), xmllint keeps parsing and may report further problems; ixmllint stops at that error, which is already fatal. And xmllint's parser does not load external DTDs, nor does ixmllint, so neither validates against a DTD; `--valid`, `--dtdvalid` and `--relaxng` have no counterpart here.

## Source Files

- Input: `.xml` and `.svg` files under `src_dirs`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

```toml
[processor.checker.ixmllint]
src_dirs = ["src"]

[processor.checker.ixmllint.presentations]
src_dirs = ["presentations"]
schema = "xsd/keynote.xsd"
dep_inputs = ["xsd/keynote.xsd"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `schema` | string | `""` | XSD schema to validate every file against, like `xmllint --schema`; relative to the project root. Empty: well-formedness only |
| `strict` | bool | `false` | Fail on namespace errors and parser warnings too (xmllint exits 0 on them) |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".xml", ".svg"]` | File extensions to check |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds (put the schema here) |

## Batch support

Files are checked individually within a batch, so one bad file fails only its own product.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
