# Advanced Usage

## Parallel builds

RSB can build independent products concurrently. Set the number of parallel jobs:

```bash
rsb build -j4       # 4 parallel jobs
rsb build -j0       # Auto-detect CPU cores
```

Or configure it in `rsb.toml`:

```toml
[build]
parallel = 4   # 0 = auto-detect
```

The `-j` flag on the command line overrides the config file setting.

## Watch mode

Watch source files and automatically rebuild on changes:

```bash
rsb watch
```

This monitors all source files and triggers an incremental build whenever a file is modified.

## Dependency graph

Visualize the build dependency graph in multiple formats:

```bash
rsb graph                    # Default text format
rsb graph --format dot       # Graphviz DOT format
rsb graph --format mermaid   # Mermaid diagram format
rsb graph --format json      # JSON format
rsb graph --view             # Open in browser or viewer
```

The `--view` flag opens the graph using the configured viewer (set in `rsb.toml`):

```toml
[graph]
viewer = "google-chrome"
```

## Ignoring files

RSB respects `.gitignore` files automatically. Any file ignored by git is also ignored by all processors. Nested `.gitignore` files and negation patterns are supported.

For project-specific exclusions that should not go in `.gitignore`, create a `.rsbignore` file in the project root with glob patterns (one per line):

```
/src/experiments/**
*.bak
```

The syntax is the same as `.gitignore`: `#` for comments, `/` prefix to anchor to the project root, `/` suffix for directories, and `*`/`**` for globs.

## Processor verbosity levels

Control the detail level of build output with `-v N`:

| Level | Output |
|---|---|
| **0** (default) | Target basename only: `main.elf` |
| **1** | Target path: `out/cc_single_file/main.elf`; cc_single_file processor also prints compiler commands |
| **2** | Adds source path: `out/cc_single_file/main.elf <- src/main.c` |
| **3** | Adds all inputs: `out/cc_single_file/main.elf <- src/main.c, src/utils.h` |

## Dry run

Preview what would be built without executing anything:

```bash
rsb build --dry-run
```

## Keep going after errors

By default, RSB stops on the first error. Use `--keep-going` to continue building other products:

```bash
rsb build --keep-going
```

## Build timings

Show per-product and total timing information:

```bash
rsb build --timings
```

## Shell completions

Generate shell completions for your shell:

```bash
rsb complete bash    # Bash completions
rsb complete zsh     # Zsh completions
rsb complete fish    # Fish completions
```

Configure which shells to generate completions for:

```toml
[completions]
shells = ["bash"]
```

## Extra inputs

By default, each processor only tracks its primary source files as inputs. If a product depends on additional files that aren't automatically discovered (e.g., a config file read by a linter, a suppressions file used by a static analyzer, or a Python settings file loaded by a template), you can declare them with `extra_inputs`.

When any file listed in `extra_inputs` changes, all products from that processor are rebuilt.

```toml
[processor.template]
extra_inputs = ["config/settings.py", "config/database.py"]

[processor.ruff]
extra_inputs = ["pyproject.toml"]

[processor.pylint]
extra_inputs = ["pyproject.toml"]

[processor.cpplint]
extra_inputs = [".cppcheck-suppressions"]

[processor.cc_single_file]
extra_inputs = ["Makefile.inc"]

[processor.spellcheck]
extra_inputs = ["custom-dictionary.txt"]
```

Paths are relative to the project root. Missing files cause a build error, so all listed files must exist.

The `extra_inputs` paths are included in the processor's config hash, so adding or removing entries triggers a rebuild even if the files themselves haven't changed. The file contents are also checksummed as part of the product's input set, so any content change is detected by the incremental build system.

All processors support `extra_inputs`.

## Graceful interrupt

Pressing Ctrl+C during a build saves progress. The next build will resume from where it left off rather than starting over.
