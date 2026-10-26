# Advanced Usage

## Parallel builds

RSConstruct can build independent products concurrently. Set the number of parallel jobs:

```bash
rsconstruct build -j4       # 4 parallel jobs
rsconstruct build -j0       # Auto-detect CPU cores
```

Or configure it in `rsconstruct.toml`:

```toml
[build]
parallel = 4   # 0 = auto-detect
```

The `-j` flag on the command line overrides the config file setting.

## Watch mode

Watch source files and automatically rebuild on changes:

```bash
rsconstruct watch
```

This monitors all source files and triggers an incremental build whenever a file is modified.

## Dependency graph

Visualize the build dependency graph in multiple formats:

```bash
rsconstruct graph                         # List the graph subcommands
rsconstruct graph show                    # Print the graph (default: SVG)
rsconstruct graph show --format text      # Plain text hierarchical view
rsconstruct graph show --format dot       # Graphviz DOT format
rsconstruct graph show --format mermaid   # Mermaid diagram format
rsconstruct graph show --format json      # JSON format
rsconstruct graph view                    # Open in a viewer (--viewer svg, the default, or mermaid)
```

`graph view` opens the graph using the configured viewer (set in `rsconstruct.toml`):

```toml
[graph]
viewer = "google-chrome"
```

## Ignoring files

RSConstruct respects `.gitignore` files automatically. Any file ignored by git is also ignored by all processors. Nested `.gitignore` files and negation patterns are supported.

For project-specific exclusions that should not go in `.gitignore`, create a `.rsconstructignore` file in the project root with glob patterns (one per line):

```text
/src/experiments/**
*.bak
```

The syntax is the same as `.gitignore`: `#` for comments, `/` prefix to anchor to the project root, `/` suffix for directories, and `*`/`**` for globs.

## Build output verbosity

By default a build prints only the phase summary lines (`[build] 1 to build, 0 to restore ...`). `-v`/`--verbose` is a boolean flag that adds one line per product, saying whether it was skipped, restored from cache, or processed:

```text
[processor.generator.cc_single_file] Restored from cache: out/processor.generator.cc_single_file/src/hello.elf <- src/hello.c
```

What those lines show for each file is controlled by three separate flags, not by a verbosity level:

| Flag | Values | Effect |
|---|---|---|
| `-O`, `--output-display` | `none` (default), `basename`, `path` | Whether and how the output file is shown |
| `-I`, `--input-display` | `none`, `source` (default), `all` | Show no inputs, only the primary source file, or every input including headers/dependencies |
| `-P`, `--path-format` | `basename`, `path` (default) | Show displayed files as bare file names or as project-relative paths |

To see the commands a processor runs, use `--show-child-processes`; to see tool output even on success, use `--show-output`. All of these are [global flags](commands.md#global-flags).

## Dry run

Preview what would be built without executing anything:

```bash
rsconstruct build --dry-run
rsconstruct build --dry-run --explain              # with the reason for each product
rsconstruct build --dry-run -p processor.checker.ruff
```

The dry run plans exactly what `build` with the same flags would: `-p`, `-x`,
`--target`, `--dir`, `--force` and `--iset`/`--pset` apply as they do to a
build. It uses the build's own prediction, so a product whose input another
product is about to rewrite is shown as `BUILD` ("a dependency will be rebuilt
or restored"), not `SKIP`. Nothing is executed and the stored config baseline
is not advanced, so the next build still reports a config change.

`rsconstruct status` answers a different question: what the cache says about
each product as it stands now, without predicting what other products would
do first.

## Keep going after errors

By default, RSConstruct stops on the first error. Use `--keep-going` to continue building other products:

```bash
rsconstruct build --keep-going
```

## Build timings

Show per-product and total timing information:

```bash
rsconstruct build --timings
```

## Shell completions

Generate shell completions for your shell:

```bash
rsconstruct complete bash    # Bash completions
rsconstruct complete zsh     # Zsh completions
rsconstruct complete fish    # Fish completions
```

Configure which shells to generate completions for:

```toml
[completions]
shells = ["bash"]
```

## Extra inputs

By default, each processor only tracks its primary source files as inputs. If a product depends on additional files that aren't automatically discovered (e.g., a config file read by a linter, a suppressions file used by a static analyzer, or a Python settings file loaded by a template), you can declare them with `dep_inputs`.

When any file listed in `dep_inputs` changes, all products from that processor are rebuilt.

```toml
[processor.generator.tera]
dep_inputs = ["config/settings.py", "config/database.py"]

[processor.checker.ruff]
dep_inputs = ["pyproject.toml"]

[processor.checker.pylint]
dep_inputs = ["pyproject.toml"]

[processor.checker.cppcheck]
dep_inputs = [".cppcheck-suppressions"]

[processor.generator.cc_single_file]
dep_inputs = ["Makefile.inc"]

[processor.checker.zspell]
dep_inputs = ["custom-dictionary.txt"]
```

Paths are relative to the project root. Missing files cause a build error, so all listed files must exist.

The `dep_inputs` paths are included in the processor's config hash, so adding or removing entries triggers a rebuild even if the files themselves haven't changed. The file contents are also checksummed as part of the product's input set, so any content change is detected by the incremental build system.

All processors support `dep_inputs`.

## Graceful interrupt

Pressing Ctrl+C during a build stops execution promptly:

- **Subprocess termination** — All external processes (compilers, linters, etc.) are spawned with a poll loop that checks for interrupts every 50ms. When Ctrl+C is detected, the running child process is killed immediately rather than waiting for it to finish. This keeps response time under 50ms regardless of how long the subprocess would otherwise run.
- **Progress preservation** — Products that completed successfully before the interrupt are cached. The next build resumes from where it left off rather than starting over.
- **Parallel builds** — In parallel mode, all in-flight subprocesses are killed when Ctrl+C is detected. Each thread's poll loop independently checks the global interrupt flag.
