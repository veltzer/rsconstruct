# PHP Lint Processor

## Purpose

Checks PHP syntax using `php -l`.

## How It Works

Discovers `.php` files in the project (excluding common build tool directories),
runs `php -l` on each file, and records success in the cache. A non-zero exit
code fails the product.

This processor supports batch mode.

## Source Files

- Input: `**/*.php`
- Output: none (checker)

## Configuration

```toml
[processor.php_lint]
args = []
extra_inputs = []
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `args` | string[] | `[]` | Extra arguments passed to php |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Batch support

The tool accepts multiple files on the command line. When batching is enabled (default), rsconstruct passes all files in a single invocation for better performance.
