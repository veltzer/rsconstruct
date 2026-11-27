# Pandoc Processor

## Purpose

Converts documents between formats using [pandoc](https://pandoc.org/).

A Rust alternative is [rspandoc](https://github.com/veltzer/rspandoc), whose
`convert` command takes pandoc's options: set `command = "rspandoc"` and leave
the rest of the stanza as it is (rsconstruct runs it as `rspandoc convert`,
which needs rspandoc 0.1.12 or later). It covers Markdown to HTML, DOCX and PDF with the options
on this page, and refuses a document or option it does not support instead of
approximating it, so a stanza that builds with it builds the same documents.
The PDF engine is still the one `pdf_engine` names.

## How It Works

Discovers `.md` files in the project (default `pandoc/` subdirectory) and runs
`pandoc` on each one, producing one output per format listed in `formats`.

## Source Files

- Input: `pandoc/**/*.md` (configurable via `src_dirs` / `src_extensions`)
- Output: `out/processor.generator.pandoc/{relative_path}.{format}` for each `format`

## Configuration

```toml
[processor.generator.pandoc]
command = "pandoc"             # Path to the pandoc executable
formats = ["pdf", "html", "docx"]
args = []                      # Extra arguments passed to pandoc
output_dir = "out/processor.generator.pandoc"
dep_inputs = []                # Additional files that trigger rebuilds when changed
pdf_engine = ""                # PDF engine; empty = pandoc default (pdflatex)
```

| Key | Type | Default | Description |
| ----- | ------ | --------- | ------------- |
| `command` | string | `"pandoc"` | Path to the pandoc executable |
| `formats` | string[] | `["pdf", "html", "docx"]` | Output formats to generate |
| `args` | string[] | `[]` | Extra arguments passed to pandoc |
| `output_dir` | string | `"out/processor.generator.pandoc"` | Base output directory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
| `pdf_engine` | string | `""` | Forwarded to `--pdf-engine=` when generating PDFs. See below. |

## PDF engine selection

Pandoc's default engine for PDF output is **pdflatex**, which only handles
8-bit-encoded text. Source files containing characters outside latin-1
(Hebrew, Greek, Cyrillic, CJK, accented Unicode like `λ`, `ש`, `Ω`, `é`) will
fail to compile under pdflatex without manual `\usepackage{...}` setup.

Set `pdf_engine` to use a Unicode-aware engine instead:

```toml
[processor.generator.pandoc]
formats = ["pdf"]
pdf_engine = "xelatex"   # or "lualatex"
```

Recognized values:

| Value | When to use |
| ------- | ------------- |
| `""` (default) | Pandoc's default (pdflatex). ASCII / latin-1 only. |
| `pdflatex` | Same as default; explicit form. |
| `xelatex` | Unicode-aware. Best general-purpose choice for non-ASCII text. |
| `lualatex` | Unicode-aware, embeds a Lua interpreter for advanced typesetting. |
| `tectonic` | Self-contained engine that downloads packages on demand. |
| `wkhtmltopdf`, `weasyprint`, `prince` | HTML-based PDF rendering pipelines. |
| `context` | ConTeXt-based typesetting. |

Notes:

- `pdf_engine` only affects products whose output format is `pdf`. For other
  formats (html, docx, etc.) the field is silently ignored.
- The engine is added to the processor's required-tools list, so
  `rsconstruct doctor` and `rsconstruct tool list-configured` will flag it as missing if
  it isn't installed: the configured one, or `pdflatex` when `pdf_engine` is empty and
  `formats` contains `pdf` (pandoc runs it either way).
- Changing `pdf_engine` invalidates the cache for affected products (it is
  part of the config-change checksum), so a rebuild is triggered automatically.
- An unknown value is rejected at config-load time with a clear error; the
  build never starts.
- The pdflatex-specific `\pdftrailerid{}` reproducibility hint is only
  injected when the engine is pdflatex (or the empty default). Other engines
  would error on it.

### Why the fleet uses xelatex, not tectonic

Every PDF stanza in the fleet sets `pdf_engine = "xelatex"`. On 2026-11-20
tectonic 0.17.0 was tried in its place, through rspandoc, on the hardest
fleet documents, against XeTeX from TeX Live 2025:

| Document | xelatex | tectonic |
| --- | --- | --- |
| data `cv.he.md` (Hebrew, babel with bidi) | 1.09 s, 5 pages | builds; same pages |
| demos-ai-coding exercise (box-drawing characters) | 0.93 s, 2 pages | builds; same pages |
| business-syllabi syllabus (azure theme fonts) | 1.05 s, 4 pages | fails |

- **The themed syllabi do not build.** The shared-themes LaTeX theme loads
  its fonts by file (`\setmainfont{Outfit}[Path=\sharedthemesfontdir, ...]`),
  with a path relative to the repository. xelatex resolves it against the
  directory it runs in; tectonic does not, and stops with
  `The font "Outfit-Regular" cannot be found`. That is every document of
  teaching-syllabi and business-syllabi. It could be made to work (an
  absolute font path, or tectonic's search-path options), but it is not a
  drop-in swap.
- **Where it builds, the output is the same.** Rendered page by page, the
  tectonic and xelatex PDFs differed in under 0.3% of pixels
  (anti-aliasing), with the same fonts and page counts.
- **It is not faster.** Tectonic runs the engine once where pandoc runs
  xelatex twice, but each of its runs is slower: with its package cache warm
  it took 1.18 s on the Hebrew CV, against xelatex's 1.09 s. The first run on
  a machine took 7.7 s, downloading packages.
- **It would add a network dependency to CI.** Tectonic fetches TeX packages
  on demand from its own server into `~/.cache/tectonic`. With a warm cache
  it built with no network at all, but a fresh CI runner starts cold: the
  build would need the network inside rsconstruct's sandbox (off by
  default, `sandbox_network`) and tectonic's server to be up. The runner's
  TeX Live has neither need.

Tectonic's advantage is not needing a TeX Live installation, and every
machine the fleet builds on has one. Reconsider only if that changes.

## Installation

`rsconstruct tool install project` downloads the statically linked Linux
binary from the pandoc GitHub release into `.rsconstruct/tools`, at the
version rsconstruct.lock pins, so every machine renders with the same
pandoc. `rsconstruct tool install machine pandoc` installs the
distribution's package with apt instead. The PDF engines (`xelatex`,
`pdflatex`) are TeX Live system packages and stay machine installs.

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

Pandoc is a Generator — see [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
