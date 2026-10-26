# Icheckstyle Processor

## Purpose

Checks Java code with [Checkstyle](https://checkstyle.org/)'s checks, in-process (no `checkstyle` program, no Java runtime to install). The Rust alternative to the [checkstyle](checkstyle.md) processor: a port of checkstyle 8.36.1, so a file gets the same messages, at the same lines and columns, and passes or fails the way `checkstyle` would exit.

## How It Works

Each file goes through what checkstyle does to it: its Java grammar (the ANTLR 2 lexer and parser checkstyle generates from `java.g`, with the same lookahead decisions, the same tree and the same parse errors), the comments and Javadoc it records on the way, and the checks the configuration names, walked over the tree as checkstyle's `TreeWalker` walks it. Messages are printed as checkstyle prints them, in its order, and a message of severity `error` fails the product:

```text
[ERROR] src/Bad.java:1: File does not end with a newline. [NewlineAtEndOfFile]
[ERROR] src/Bad.java:1:8: Unused import - java.util.List. [UnusedImports]
[ERROR] src/Bad.java:3:14: Should use uppercase 'L'. [UpperEll]
```

Messages of severity `warning` and `info` are printed and the file passes; `ignore` hides them. A file checkstyle cannot check (a syntax error, or one of the cases where checkstyle itself throws) fails with the exception checkstyle stops on:

```text
src/Broken.java:2:13: unexpected token: {
```

The checks are configured as checkstyle is, by a `checkstyle.xml`:

```toml
[processor.checker.icheckstyle]
src_dirs = ["src"]
```

### Implemented modules

`Checker` (with `severity`, `tabWidth`, `fileExtensions`, `charset` UTF-8), `TreeWalker`, `NewlineAtEndOfFile`, `ArrayTypeStyle`, `CovariantEquals`, `DefaultComesLast`, `EmptyStatement`, `EqualsHashCode`, `FallThrough`, `ModifiedControlVariable`, `OneStatementPerLine`, `RedundantImport`, `SimplifyBooleanExpression`, `SimplifyBooleanReturn`, `StringLiteralEquality`, `UnusedImports` and `UpperEll`, each with all its properties, `severity`, `id`, `tabWidth` and `<message>` overrides. These are the checks of the fleet's shared `checkstyle.xml`. Any other module, or a property this port does not reproduce, is a configuration error that names it, so a configuration either runs as checkstyle runs it or does not run. One property is checked more strictly than checkstyle checks it: a `tabWidth` of zero or less is a configuration error here, where checkstyle accepts it and then throws `ArithmeticException` on the first tab it meets.

### Fidelity

Checked against checkstyle 8.36.1 over 5169 Java files (the fleet's 2559, checkstyle's own sources and the 1700 Java inputs of its test suite, many of them deliberately odd syntax) and 6000 mutated copies (broken quotes, comments, braces and literals, deleted, duplicated and swapped lines), under the fleet's configuration and two configurations that set every other property: the same messages, line for line, and for every file checkstyle stops on, the same exception (about 4400 of them, parse errors and checkstyle's own exceptions alike).

It is also lighter: those 5169 files take under a second of CPU, against checkstyle's 25 seconds (and its JVM).

The engine's unit tests (`src/engines/checkstyle/tests.rs`) keep it that way: they run `checkstyle` 8.36.1 itself (a missing or different version fails them) and the engine over the files in `src/engines/checkstyle/testdata/` and deterministic mutants of them, under three configurations, and require the same result for every file.

Known differences: checkstyle checks the files of a run in one process, and two checks carry a little state from one file to the next (the line of the last lambda in `OneStatementPerLine`); each file here is checked on its own. Messages are English only, `${property}` references resolve only through a `default`, and files are read as UTF-8.

## Source Files

- Input: `.java` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `config_file` | string | `"checkstyle.xml"` | The checkstyle configuration, as `checkstyle -c`; relative to the project root |
| `dep_auto` | string[] | `["checkstyle.xml"]` | Config files that are inputs when they exist |

The configuration is an input of every product, whether it is the default `checkstyle.xml` (a `dep_auto` input, tracked when it exists) or a file named by `config_file`: a change to it re-checks every file, with no `dep_inputs` entry needed. A `config_file` other than the default must exist; a missing one fails the build naming it.

Switching a repo from checkstyle: `[processor.checker.checkstyle]` → `[processor.checker.icheckstyle]`, and `args = ["-c", "checkstyle.xml"]` is dropped (or becomes `config_file` for another file name).

## Batch support

Not used: checking in-process has no start-up cost for a batch to share, so files are checked in parallel as separate products.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
