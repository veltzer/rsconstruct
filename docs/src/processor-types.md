# Processor Types

Every processor in RSConstruct has a type that determines how it discovers inputs, produces outputs, and interacts with the cache. There are four types.

Run `rsconstruct processors types` to list them.

## Checker

A checker validates input files without producing any output. If the check passes, the result is cached — if the inputs haven't changed on the next build, the check is skipped entirely.

### How it works

1. Scans for files matching `src_extensions` in `src_dirs`
2. Creates one product per input file
3. Runs the tool on each file (or batch of files)
4. If the tool exits successfully, records a marker in the cache
5. On the next build, if inputs are unchanged, the check is skipped

### What gets cached

A marker entry — no files, no blobs. The marker's presence means "this check passed with these inputs."

### Examples

**Lint Python files with ruff:**

```toml
[processor.ruff]
```

Scans for `.py` files, runs `ruff check` on each. No output files produced.

```
src/main.py → (checker)
src/utils.py → (checker)
```

**Lint shell scripts:**

```toml
[processor.shellcheck]
```

Scans for `.sh` and `.bash` files, runs `shellcheck` on each.

**Validate YAML files:**

```toml
[processor.yamllint]
```

Scans for `.yml` and `.yaml` files, runs `yamllint` on each.

**Validate JSON files with jq:**

```toml
[processor.jq]
```

Scans for `.json` files, validates each with `jq`.

**Spell check Markdown files:**

```toml
[processor.zspell]
```

Scans for `.md` files, checks spelling with the built-in zspell engine.

### Built-in checkers

ruff, pylint, mypy, pyrefly, black, pytest, doctest, shellcheck, luacheck, yamllint, jq, jsonlint, taplo, cppcheck, clang_tidy, cpplint, checkpatch, mdl, markdownlint, rumdl, aspell, zspell, ascii, encoding, duplicate_files, terms, eslint, jshint, standard, htmlhint, htmllint, tidy, stylelint, jslint, svglint, perlcritic, xmllint, checkstyle, php_lint, yq, hadolint, slidev, json_schema, iyamlschema, ijq, ijsonlint, iyamllint, itaplo, marp_images, license_header

## Generator

A generator transforms each input file into one or more output files. It creates one product per input file (or one per input x format pair for multi-format generators like pandoc).

### How it works

1. Scans for files matching `src_extensions` in `src_dirs`
2. For each input file, computes the output path from the input path, output directory, and format
3. Creates one product per input x format pair
4. Runs the tool to produce the output file
5. Stores the output as a content-addressed blob in the cache

### What gets cached

One blob per output file. The blob is the raw file content, stored by its SHA-256 hash. On restore, the blob is hardlinked (or copied) to the output path.

### Examples

**Render Tera templates:**

```toml
[processor.tera]
```

Scans `tera.templates/` for `.tera` files, renders each template. The output path is the template path with the `.tera` extension stripped:

```
tera.templates/config.py.tera → config.py
tera.templates/README.md.tera → README.md
```

**Convert Marp slides to PDF:**

```toml
[processor.marp]
```

Scans `marp/` for `.md` files, converts each to PDF (and optionally other formats):

```
marp/slides.md → out/marp/slides.pdf
marp/intro.md → out/marp/intro.pdf
```

**Convert documents with pandoc (multi-format):**

```toml
[processor.pandoc]
```

Scans `pandoc/` for `.md` files, converts each to PDF, HTML, and DOCX. Each format is a separate product with its own cache entry:

```
pandoc/syllabus.md → out/pandoc/syllabus.pdf
pandoc/syllabus.md → out/pandoc/syllabus.html
pandoc/syllabus.md → out/pandoc/syllabus.docx
```

**Compile single-file C programs:**

```toml
[processor.cc_single_file]
```

Scans `src/` for `.c` and `.cc` files, compiles each into an executable:

```
src/main.c → out/cc_single_file/src/main.elf
src/test.c → out/cc_single_file/src/test.elf
```

**Convert Mermaid diagrams:**

```toml
[processor.mermaid]
```

Scans for `.mmd` files, converts each to PNG (configurable formats):

```
diagrams/flow.mmd → out/mermaid/diagrams/flow.png
```

**Compile SCSS to CSS:**

```toml
[processor.sass]
```

Scans `sass/` for `.scss` and `.sass` files, compiles each to CSS:

```
sass/styles.scss → out/sass/styles.css
```

### Built-in generators

tera, mako, jinja2, cc_single_file, pandoc, marp, mermaid, drawio, chromium, libreoffice, protobuf, sass, markdown2html, pdflatex, a2x, objdump, rust_single_file, tags, pdfunite, ipdfunite, imarkdown2html, isass, yaml2json, generator, script

## Creator

A creator runs a command and caches declared output files and directories. It scans for anchor files — files whose presence means "run this tool here." One product is created per anchor file found, and the command runs in the anchor file's directory.

Unlike generators (where outputs are derived from input paths), creator outputs are declared explicitly in the config via `output_dirs` and `output_files`.

### How it works

1. Scans for anchor files matching `src_extensions` in `src_dirs`
2. Creates one product per anchor file
3. Runs the command in the anchor file's directory
4. Walks all declared `output_dirs` and collects `output_files`
5. Stores each file as a content-addressed blob
6. Records a tree in the cache — a manifest listing every output file with its path, blob checksum, and Unix permissions

### What gets cached

A tree entry listing all output files. On restore, the directory tree is recreated from cached blobs with permissions preserved. Individual files within the tree that already exist with the correct checksum are skipped.

### Examples

**Install Python dependencies with pip:**

```toml
[processor.creator.venv]
command = "pip"
args = ["install", "-r", "requirements.txt"]
src_extensions = ["requirements.txt"]
output_dirs = [".venv"]
```

Scans for `requirements.txt` files. For each one, runs `pip install` and caches the entire `.venv/` directory. After `rsconstruct clean`, the venv is restored from cache instead of reinstalling.

**Build a Node.js project:**

```toml
[processor.creator.npm_build]
command = "npm"
args = ["run", "build"]
src_extensions = ["package.json"]
output_dirs = ["dist"]
```

Scans for `package.json` files, runs `npm run build`, caches the `dist/` directory.

**Build documentation with Sphinx:**

```toml
[processor.sphinx]
```

Scans for `conf.py` files, runs `sphinx-build`, caches the output directory.

```
docs/conf.py → (creator)
```

**Build a Rust project with Cargo:**

```toml
[processor.cargo]
```

Scans for `Cargo.toml` files, runs `cargo build`, optionally caches the `target/` directory.

```
Cargo.toml → (creator)
```

**Run a custom build script:**

```toml
[processor.creator.assets]
command = "./build_assets.sh"
src_extensions = [".manifest"]
src_dirs = ["."]
output_dirs = ["assets/compiled", "assets/sprites"]
output_files = ["assets/manifest.json"]
```

Scans for `.manifest` files, runs the build script, caches two output directories and one output file.

### Built-in creators

cargo, pip, npm, gem, sphinx, mdbook, jekyll, cc (full C/C++ projects)

User-defined creators use the `creator` processor type directly via `[processor.creator.NAME]`.

## Explicit

An explicit processor aggregates many inputs into (possibly) many output files and/or directories. Unlike other types which create one product per discovered file, explicit creates a single product with all declared inputs and outputs.

### How it works

1. Inputs are listed explicitly via `inputs` and `input_globs` in the config
2. Creates a single product with all inputs and all outputs
3. Runs the command, passing `--inputs` and `--outputs` on the command line
4. Stores each output file as a content-addressed blob

### What gets cached

One blob per output file (like generator).

### Examples

**Build a static site from generated HTML:**

```toml
[processor.explicit.site]
command = "python3"
args = ["build_site.py"]
input_globs = ["out/pandoc/*.html", "templates/*.html"]
inputs = ["site.yaml"]
outputs = ["out/site/index.html", "out/site/style.css"]
```

Waits for pandoc to produce HTML files, then combines them with templates into a site. All inputs are aggregated into one product:

```
out/pandoc/page1.html, out/pandoc/page2.html, templates/base.html, site.yaml → out/site/index.html, out/site/style.css
```

**Merge PDFs into a course bundle:**

```toml
[processor.explicit.course]
command = "pdfunite"
input_globs = ["out/pdflatex/*.pdf"]
outputs = ["out/course/full-course.pdf"]
```

Aggregates all PDF outputs from pdflatex into a single merged PDF.

### Built-in explicit processors

explicit, pdfunite, ipdfunite

## Comparison

| | Checker | Generator | Creator | Explicit |
|---|---|---|---|---|
| **Purpose** | Validate | Transform | Build/install | Aggregate |
| **Inputs** | Scanned | Scanned | Scanned (anchor files) | Declared in config |
| **Products** | One per input | One per input (x format) | One per anchor | One total |
| **Outputs** | None | Derived from input path | Declared dirs + files | Declared files |
| **Cache type** | Marker | Blob | Tree | Blob |
| **Runs in** | Project root | Project root | Anchor file's directory | Project root |
| **Command args** | Input files | Input + output | User-defined args | `--inputs` + `--outputs` |
