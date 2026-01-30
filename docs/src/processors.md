# Processors

RSB uses **processors** to discover and build products. Each processor scans for source files matching its conventions and produces output files.

Enable processors in `rsb.toml`:

```toml
[processor]
enabled = ["template", "pylint", "cc", "cpplint", "spellcheck"]
```

Use `rsb processor list` to see available processors and their status.

## Available Processors

- [Template](processors/template.md) — renders Tera templates into output files
- [Pylint](processors/pylint.md) — lints Python source files
- [CC](processors/cc.md) — compiles C/C++ source files into executables
- [Cpplint](processors/cpplint.md) — runs static analysis on C/C++ source files
- [Spellcheck](processors/spellcheck.md) — checks documentation files for spelling errors
- [Sleep](processors/sleep.md) — sleeps for a duration (for testing)
