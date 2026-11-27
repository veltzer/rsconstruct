# Vnu Processor

## Purpose

Validates HTML files with the [Nu Html Checker](https://validator.github.io/validator/)
(vnu), the checker behind [validator.w3.org](https://validator.w3.org/nu/).

It checks a document against the HTML standard, content model included: a
`<main>` inside a `<main>`, a `<style>` in the body, an element where its
parent does not allow it, a missing required attribute. [`tidy`](tidy.md)
and [`itidy`](itidy.md) do not know those rules, so the two complement each
other: tidy for malformed markup, vnu for valid HTML.

## How It Works

Discovers `.html` and `.htm` files in the project (excluding common build tool
directories), runs `java -jar <vnu.jar>` on them, and records success in the
cache. A non-zero exit code from the checker fails the product. The checker
exits non-zero on errors only; warnings are printed and pass, unless
`--Werror` is given.

This processor supports batch mode, and batching matters here: every
invocation starts a Java virtual machine.

## Tools

| Tool | Installed by | What it is |
| ------ | -------------- | ------------ |
| `vnu.jar` | `binary` (into the project or the machine) | The checker, one jar, fetched from the npm registry's `vnu-jar` package and locked in `rsconstruct.lock` |
| `java` | `apt` (`default-jre-headless`, machine only) | The Java runtime that runs the jar |

The jar is found on `PATH` like any other tool and handed to `java -jar` by
its full path.

Upstream also publishes a self-contained Linux build that needs no Java
(`vnu.linux.zip`), but only under a GitHub release tag, `latest`, whose
assets are replaced in place on every upstream commit. A lock file cannot
name such a download: its hash would be wrong within days. The npm registry
keeps every version of the jar under a permanent URL, which is why the jar
is what gets installed.

## Source Files

- Input: `**/*.html`, `**/*.htm`
- Output: none (checker)

## Configuration

```toml
[processor.checker.vnu]
command = "java"
args = ["--errors-only"]
dep_inputs = []
```

| Key | Type | Default | Description |
| ----- | ------ | --------- | ------------- |
| `command` | string | `"java"` | The Java runtime that runs the checker |
| `args` | string[] | `[]` | Extra arguments passed to the checker, after `-jar <vnu.jar>` |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Useful checker arguments:

- `--errors-only`: do not print warnings.
- `--Werror`: treat warnings as errors.
- `--filterfile <file>` or `--filterpattern <regex>`: drop the messages that
  match, for findings a project accepts. List the filter file in
  `dep_inputs` so that editing it re-runs the check.
- `--also-check-css`, `--also-check-svg`: check those files too, when
  `src_extensions` is widened to include them.

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
