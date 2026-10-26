# tera

Scans Tera template files for `{% include %}`, `{% import %}`, and `{% extends %}` directives and adds referenced template files as dependencies.

**Builtin**: Yes.

**Auto-detects**: Projects with `.tera` files.

## Features

- Extracts paths from `{% include "path" %}`, `{% import "path" %}`, and `{% extends "path" %}`
- Handles both double- and single-quoted paths
- Tracks the file read by `load_python`, `load_lua`, `load_data`, `load_json`, `load_toml`, `load_csv`, `toml_get` (their `path=` argument) and `version_str` (`path=`, or `config/version.py` when called without one) as a content-tracked input
- Mixes the resolved file set of `glob`, `git_count_files`, `grep_count` and `workflow_names` and the command text of `shell_output` into the product's cache key (see the [tera processor](../processor/generator/tera.md) for what each function does)
- Resolves a template or data file that another product generates, before it exists: the template is rendered after its producer (see [Generated files](../analyzers.md#generated-files))

This ensures that when an included template changes, any template that includes it is rebuilt.

## Path resolution

Every path is resolved against the project root and nothing else, exactly as the tera processor resolves it: the processor registers each includable template under its project-relative path (so `{% include "tera.snippets/footer.tera" %}` is the only spelling that renders), and the loader functions open `path=` relative to the directory rsconstruct runs in. A path relative to the template's own directory is neither rendered nor tracked. The analyzer resolving a path the processor would not read (or the reverse) would track the wrong file, so the two deliberately share one rule.

## Function arguments

The analyzer reads function calls the way the Tera parser does: arguments are `name=value` pairs in any order, string literals may use `"`, `'` or backticks, and a `)` or `,` inside a string literal is part of the string. `shell_output(command="echo $(uname -r)", depends_on=["conf/*"])` is therefore a complete call with its `depends_on`. Only literal values can be tracked: `shell_output` requires `depends_on` to be a literal list of glob patterns, and a `path=` or `pattern=` that is a variable is not followed.

## Configuration

```toml
[analyzer.tera]
enabled = true   # the only field; false keeps the stanza but skips the analyzer
```
