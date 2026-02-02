# Processors

RSB uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

Enable processors in `rsb.toml`:

```toml
[processor]
enabled = ["template", "ruff", "pylint", "cc_single_file", "cpplint", "spellcheck", "sleep", "make"]
```

Use `rsb processor list` to see available processors and their status.
Use `rsb processor all` to see all processors with descriptions.
Use `rsb processor auto` to see which processors are auto-detected for the current project.
Use `rsb processor files` to see which files each processor discovers.

## Available Processors

- [Template](processors/template.md) — renders Tera templates into output files
- [Ruff](processors/ruff.md) — lints Python files with ruff
- [Pylint](processors/pylint.md) — lints Python files with pylint
- [CC Single File](processors/cc.md) — compiles C/C++ source files into executables (single-file)
- [Cpplint](processors/cpplint.md) — runs static analysis on C/C++ source files
- [Spellcheck](processors/spellcheck.md) — checks documentation files for spelling errors
- [Sleep](processors/sleep.md) — sleeps for a duration (for testing)
- [Make](processors/make.md) — runs make in directories containing Makefiles

## Custom Processors

You can define custom processors in Lua. See [Lua Plugins](plugins.md) for details.
