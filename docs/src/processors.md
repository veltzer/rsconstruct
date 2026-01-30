# Processors

RSB uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

Enable processors in `rsb.toml`:

```toml
[processor]
enabled = ["template", "ruff", "pylint", "cc", "cpplint", "spellcheck"]
```

Use `rsb processor list` to see available processors and their status.
Use `rsb processor all` to see all processors with descriptions.

## Available Processors

- [Template](processors/template.md) — renders Tera templates into output files
- [Ruff](processors/ruff.md) — lints Python files with ruff
- [Pylint](processors/pylint.md) — lints Python files with pylint
- [CC](processors/cc.md) — compiles C/C++ source files into executables
- [Cpplint](processors/cpplint.md) — runs static analysis on C/C++ source files
- [Spellcheck](processors/spellcheck.md) — checks documentation files for spelling errors
- [Sleep](processors/sleep.md) — sleeps for a duration (for testing)
