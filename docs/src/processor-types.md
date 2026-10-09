# Processor Types

Every processor in RSConstruct has a type that determines how it discovers inputs, produces outputs, and interacts with the cache. There are five types.

Run `rsconstruct processor types` to list them.

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
[processor.checker.ruff]
```

Scans for `.py` files, runs `ruff check` on each. No output files produced.

```text
src/main.py → (checker)
src/utils.py → (checker)
```

**Lint shell scripts:**

```toml
[processor.checker.shellcheck]
```

Scans for `.sh` and `.bash` files, runs `shellcheck` on each.

**Validate YAML files:**

```toml
[processor.checker.yamllint]
```

Scans for `.yml` and `.yaml` files, runs `yamllint` on each.

**Validate JSON files with jq:**

```toml
[processor.checker.jq]
```

Scans for `.json` files, validates each with `jq`.

**Spell check Markdown files:**

```toml
[processor.checker.zspell]
```

Scans for `.md` files, checks spelling with the built-in zspell engine.

### Built-in checkers

actionlint, ascii, aspell, biome, black, checkpatch, checkstyle, clang_tidy, clippy, cmake, cppcheck, cpplint, doctest, duplicate_files, encoding, eslint, hadolint, htmlhint, htmllint, iactionlint, ijq, ijsonlint, isvglint, itaplo, itidy, ixmllint, iyamllint, iyamlschema, jq, jshint, jslint, json_schema, jsonlint, license_header, luacheck, make, markdownlint, marp_images, mdl, mypy, oxlint, perlcritic, php_lint, prettier, pylint, pyrefly, pytest, ruff, rumdl, script, shellcheck, slidev, standard, stylelint, svglint, svgo, taplo, terms, tidy, xmllint, yamllint, yq, zspell

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
[processor.generator.tera]
```

Scans `tera.templates/` for `.tera` files, renders each template. The output path is the template path with the `.tera` extension stripped:

```text
tera.templates/config.py.tera → config.py
tera.templates/README.md.tera → README.md
```

**Convert Marp slides to PDF:**

```toml
[processor.generator.marp]
```

Scans `marp/` for `.md` files, converts each to PDF (and optionally other formats):

```text
marp/slides.md → out/processor.generator.marp/slides.pdf
marp/intro.md → out/processor.generator.marp/intro.pdf
```

**Convert documents with pandoc (multi-format):**

```toml
[processor.generator.pandoc]
```

Scans `pandoc/` for `.md` files, converts each to PDF, HTML, and DOCX. Each format is a separate product with its own cache entry:

```text
pandoc/syllabus.md → out/processor.generator.pandoc/syllabus.pdf
pandoc/syllabus.md → out/processor.generator.pandoc/syllabus.html
pandoc/syllabus.md → out/processor.generator.pandoc/syllabus.docx
```

**Compile single-file C programs:**

```toml
[processor.generator.cc_single_file]
```

Scans `src/` for `.c` and `.cc` files, compiles each into an executable:

```text
src/main.c → out/processor.generator.cc_single_file/src/main.elf
src/test.c → out/processor.generator.cc_single_file/src/test.elf
```

**Convert Mermaid diagrams:**

```toml
[processor.generator.mermaid]
```

Scans for `.mmd` files, converts each to PNG (configurable formats):

```text
diagrams/flow.mmd → out/processor.generator.mermaid/diagrams/flow.png
```

**Compile SCSS to CSS:**

```toml
[processor.generator.sass]
```

Scans `sass/` for `.scss` and `.sass` files, compiles each to CSS:

```text
sass/styles.scss → out/processor.generator.sass/styles.css
```

### Built-in generators

a2x, cc_single_file, chromium, drawio, generic, imarkdown2html, ipdfunite, isass, jinja2, libreoffice, mako, markdown2html, marp, mermaid, objdump, pandoc, pdflatex, pdfunite, protobuf, requirements, rust_single_file, sass, tags, tera, yaml2json

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
[processor.creator.generic.venv]
command = "pip"
args = ["install", "-r", "requirements.txt"]
src_extensions = ["requirements.txt"]
output_dirs = [".venv"]
```

Scans for `requirements.txt` files. For each one, runs `pip install` and caches the entire `.venv/` directory. After `rsconstruct clean`, the venv is restored from cache instead of reinstalling.

**Build a Node.js project:**

```toml
[processor.creator.generic.npm_build]
command = "npm"
args = ["run", "build"]
src_extensions = ["package.json"]
output_dirs = ["dist"]
```

Scans for `package.json` files, runs `npm run build`, caches the `dist/` directory.

**Build documentation with Sphinx:**

```toml
[processor.creator.sphinx]
```

Scans for `conf.py` files, runs `sphinx-build`, caches the output directory.

```text
docs/conf.py → (creator)
```

**Build a Rust project with Cargo:**

```toml
[processor.creator.cargo]
```

Scans for `Cargo.toml` files, runs `cargo build`, optionally caches the `target/` directory.

```text
Cargo.toml → (creator)
```

**Run a custom build script:**

```toml
[processor.creator.generic.assets]
command = "./build_assets.sh"
src_extensions = [".manifest"]
src_dirs = ["."]
output_dirs = ["assets/compiled", "assets/sprites"]
output_files = ["assets/manifest.json"]
```

Scans for `.manifest` files, runs the build script, caches two output directories and one output file.

### Built-in creators

cargo, cc, gem, generic, jekyll, linux_module, mdbook, npm, pip, sphinx

User-defined creators use the config-driven `generic` creator via `[processor.creator.generic.NAME]`.

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
[processor.explicit.generic.site]
command = "python3"
args = ["build_site.py"]
input_globs = ["out/processor.generator.pandoc/*.html", "templates/*.html"]
inputs = ["site.yaml"]
outputs = ["out/site/index.html", "out/site/style.css"]
```

Waits for pandoc to produce HTML files, then combines them with templates into a site. All inputs are aggregated into one product:

```text
out/processor.generator.pandoc/page1.html, out/processor.generator.pandoc/page2.html, templates/base.html, site.yaml → out/site/index.html, out/site/style.css
```

**Merge PDFs into a course bundle:**

```toml
[processor.explicit.generic.course]
command = "pdfunite"
input_globs = ["out/processor.generator.pdflatex/*.pdf"]
outputs = ["out/course/full-course.pdf"]
```

Aggregates all PDF outputs from pdflatex into a single merged PDF.

### Built-in explicit processors

generic

## Mass Generator

A mass generator runs a tool that produces many files in one invocation and can enumerate them in advance. It is the transparent creator: where a creator declares an opaque `output_dirs`, a mass generator asks the tool for a manifest first and turns every predicted file into a product of its own.

### How it works

1. At graph-build time, runs `predict_command` and parses the JSON manifest it prints
2. Creates one product per manifest entry: `inputs` = the entry's `sources`, `outputs` = its `path`
3. Classifies each product individually (skip, restore, or build)
4. The first product that needs building runs `command`; the rest find the tool already run
5. Compares the files the tool wrote against the plan — a missing predicted file or an off-plan write fails the build
6. Stores each predicted file as a content-addressed blob

### What gets cached

One blob per predicted file (like generator). When every product is clean the tool does not run at all; when some are, the tool runs once and every dirty product caches its own file.

### Examples

**A static site generator with a plan mode:**

```toml
[processor.mass_generator.generic.site]
command         = "rssite"
args            = ["build"]
predict_command = "rssite"
predict_args    = ["plan"]
output_dirs     = ["_site"]
```

`rssite plan` prints which `_site/*.html` files `rssite build` will write and which sources each depends on. Changing `docs/about.md` dirties only `_site/about/index.html`; a linter scanning `_site/` depends on each page individually.

```text
docs/index.md, templates/default.html → _site/index.html
docs/about.md, templates/default.html → _site/about/index.html
```

### Built-in mass generators

generic, zola

Any tool that honors the manifest contract plugs into the config-driven `generic` mass generator via `[processor.mass_generator.generic.NAME]`; see [Mass Generator](processor/mass_generator/generic.md).

## Comparison

| | Checker | Generator | Creator | Explicit | Mass Generator |
|---|---|---|---|---|---|
| **Purpose** | Validate | Transform | Build/install | Aggregate | Transform, many at once |
| **Inputs** | Scanned | Scanned | Scanned (anchor files) | Declared in config | Declared by the tool's plan |
| **Products** | One per input | One per input (x format) | One per anchor | One total | One per predicted file |
| **Outputs** | None | Derived from input path | Declared dirs + files | Declared files | Predicted files |
| **Cache type** | Marker | Blob | Tree | Blob | Blob |
| **Runs in** | Project root | Project root | Anchor file's directory | Project root | Project root |
| **Command args** | Input files | Input + output | User-defined args | `--inputs` + `--outputs` | User-defined args |
