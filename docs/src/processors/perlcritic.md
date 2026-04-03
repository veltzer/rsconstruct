# Perlcritic Processor

## Purpose

Analyzes Perl code using [Perl::Critic](https://metacpan.org/pod/Perl::Critic).

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
[processor.perlcritic]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to perlcritic |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
