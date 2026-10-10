# Perlcritic Processor

## Purpose

Analyzes Perl code using [Perl::Critic](https://metacpan.org/pod/Perl::Critic).

The Rust alternative is [iperlcritic](iperlcritic.md): PPI and the severity-5 policies, in-process, with perlcritic's violations and positions. Switching is `perlcritic` → `iperlcritic` in the stanza header; `.perlcriticrc` is read as before.

## How It Works

Discovers `.pl` and `.pm` files in the project (excluding common build tool
directories), runs `perlcritic` on each file, and records success in the cache.
A non-zero exit code from perlcritic fails the product.

This processor supports batch mode.

If a `.perlcriticrc` file exists, it is automatically added as an extra input so
that configuration changes trigger rebuilds.

## Source Files

- Input: `**/*.pl`, `**/*.pm`
- Output: none (checker)

## Configuration

```toml
[processor.checker.perlcritic]
args = []
dep_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to perlcritic |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
