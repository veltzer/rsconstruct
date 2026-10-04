# Processor Base Struct Refactor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate per-processor boilerplate by introducing a `ProcessorBase` struct that provides default implementations for `config_json()`, `max_jobs()`, `clean()`, `processor_type()`, `description()`, and `auto_detect()`.

**Architecture:** Add a `ProcessorBase` struct holding common fields (config as `serde_json::Value`, processor name, description, processor type, scan config ref). Each processor embeds `ProcessorBase` and delegates boilerplate trait methods to it. A `delegate_base!` macro generates the delegation one-liners. Processors only implement `execute()`, `discover()`, and `required_tools()` manually — everything else comes from the base.

**Tech Stack:** Rust, serde_json, existing `ProductDiscovery` trait

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/processor/base.rs` | Create | `ProcessorBase` struct + methods for all boilerplate |
| `src/processor/mod.rs` | Modify | Add `mod base`, `pub use base::ProcessorBase`, add `delegate_base!` macro |
| `src/processor/generator/mod.rs` | Modify | Replace `impl_generator!` macro with `delegate_base!` |
| `src/processor/generator/marp.rs` | Modify | First migration: use `ProcessorBase` |
| `src/processor/generator/mako.rs` | Modify | Migration |
| `src/processor/generator/jinja2.rs` | Modify | Migration |
| `src/processor/generator/tera.rs` | Modify | Migration |
| `src/processor/generator/pdflatex.rs` | Modify | Migration |
| `src/processor/generator/pdfunite.rs` | Modify | Migration |
| `src/processor/generator/rust_single_file.rs` | Modify | Migration |
| `src/processor/generator/generator.rs` | Modify | Migration |
| `src/processor/generator/tags.rs` | Modify | Migration |
| `src/processor/generator/cc_single_file/mod.rs` | Modify | Migration |
| `src/processor/generator/linux_module.rs` | Modify | Migration |
| `src/processor/generator/explicit.rs` | Modify | Migration (partial - no standard config) |
| `src/processor/generator/pandoc.rs` | Modify | Migration from `impl_generator!` |
| `src/processor/generator/sass.rs` | Modify | Migration from `impl_generator!` |
| `src/processor/generator/chromium.rs` | Modify | Migration from `impl_generator!` |
| (remaining impl_generator! users) | Modify | Migration from `impl_generator!` |
| `src/processor/checker/mod.rs` | Modify | Update `impl_checker!` to use `ProcessorBase` |
| `src/processor/checker/zspell.rs` | Modify | Migration of manual checker |
| `src/processor/checker/clippy.rs` | Modify | Migration of manual checker |
| `src/processor/checker/make.rs` | Modify | Migration of manual checker |
| `src/processor/checker/aspell.rs` | Modify | Migration of manual checker |
| `src/processor/checker/mdl.rs` | Modify | Migration of manual checker |
| `src/processor/checker/markdownlint.rs` | Modify | Migration of manual checker |
| `tests/tests_mod/build.rs` | Verify | Run existing tests to confirm no regressions |

---

### Task 1: Create ProcessorBase struct

**Files:**
- Create: `src/processor/base.rs`
- Modify: `src/processor/mod.rs`

- [ ] **Step 1: Create `src/processor/base.rs` with the base struct**

```rust
use serde::Serialize;
use crate::config::ScanConfig;
use crate::graph::Product;
use crate::processors::ProcessorType;

/// Common base for all processors. Holds fields needed by boilerplate
/// ProductDiscovery methods so each processor doesn't repeat them.
pub struct ProcessorBase {
    /// Processor name constant (e.g., "marp", "pylint")
    pub name: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Generator or Checker
    pub processor_type: ProcessorType,
}

impl ProcessorBase {
    pub fn generator(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Generator }
    }

    pub fn checker(name: &'static str, description: &'static str) -> Self {
        Self { name, description, processor_type: ProcessorType::Checker }
    }

    pub fn description(&self) -> &str {
        self.description
    }

    pub fn processor_type(&self) -> ProcessorType {
        self.processor_type
    }

    pub fn config_json<C: Serialize>(config: &C) -> Option<String> {
        serde_json::to_string(config).ok()
    }

    pub fn clean(product: &Product, name: &str, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::clean_outputs(product, name, verbose)
    }

    pub fn auto_detect(scan: &ScanConfig, file_index: &crate::file_index::FileIndex) -> bool {
        crate::processors::scan_root_valid(scan) && !file_index.scan(scan, true).is_empty()
    }
}
```

- [ ] **Step 2: Add `mod base` and `pub use` to `src/processor/mod.rs`**

Add near the top of `src/processor/mod.rs`, after the other `mod` declarations:

```rust
mod base;
pub use base::ProcessorBase;
```

- [ ] **Step 3: Add `delegate_base!` macro to `src/processor/mod.rs`**

Add after the `pub use base::ProcessorBase;` line:

```rust
/// Macro to delegate boilerplate ProductDiscovery methods to ProcessorBase.
/// Usage inside `impl ProductDiscovery for MyProcessor { delegate_base!(); ... }`
///
/// Delegates: description, processor_type, config_json, max_jobs, clean (generators only).
/// The processor must have:
///   - `self.base: ProcessorBase`
///   - `self.config` implementing `Serialize` and having `max_jobs: Option<usize>`
///   - `self.config.scan: ScanConfig`
#[macro_export]
macro_rules! delegate_base {
    // Generator variant: includes clean()
    (generator) => {
        fn description(&self) -> &str {
            self.base.description()
        }

        fn processor_type(&self) -> $crate::processors::ProcessorType {
            self.base.processor_type()
        }

        fn auto_detect(&self, file_index: &$crate::file_index::FileIndex) -> bool {
            $crate::processors::ProcessorBase::auto_detect(&self.config.scan, file_index)
        }

        fn config_json(&self) -> Option<String> {
            $crate::processors::ProcessorBase::config_json(&self.config)
        }

        fn max_jobs(&self) -> Option<usize> {
            self.config.max_jobs
        }

        fn clean(&self, product: &$crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
            $crate::processors::ProcessorBase::clean(product, self.base.name, verbose)
        }
    };

    // Checker variant: no clean() (default returns Ok(0))
    (checker) => {
        fn description(&self) -> &str {
            self.base.description()
        }

        fn processor_type(&self) -> $crate::processors::ProcessorType {
            self.base.processor_type()
        }

        fn auto_detect(&self, file_index: &$crate::file_index::FileIndex) -> bool {
            $crate::processors::ProcessorBase::auto_detect(&self.config.scan, file_index)
        }

        fn config_json(&self) -> Option<String> {
            $crate::processors::ProcessorBase::config_json(&self.config)
        }

        fn max_jobs(&self) -> Option<usize> {
            self.config.max_jobs
        }
    };
}
```

- [ ] **Step 4: Build and verify compilation**

Run: `cargo build`
Expected: Compiles with no errors (macro and struct are defined but not yet used)

- [ ] **Step 5: Commit**

```bash
git add src/processor/base.rs src/processor/mod.rs
git commit -m "feat: add ProcessorBase struct and delegate_base! macro"
```

---

### Task 2: Migrate MarpProcessor (first manual generator)

**Files:**
- Modify: `src/processor/generator/marp.rs`

- [ ] **Step 1: Run existing tests to establish baseline**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 2: Rewrite MarpProcessor to use ProcessorBase**

Replace the full contents of `src/processor/generator/marp.rs` with:

```rust
use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::MarpConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, run_command, check_command_output};

use super::DiscoverParams;

fn cleanup_marp_tmp_dirs() {
    let Ok(entries) = fs::read_dir("/tmp") else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().starts_with("marp-cli-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub struct MarpProcessor {
    base: ProcessorBase,
    config: MarpConfig,
}

impl MarpProcessor {
    pub fn new(config: MarpConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::MARP,
                "Convert Marp slides to PDF/PPTX/HTML",
            ),
            config,
        }
    }
}

impl ProductDiscovery for MarpProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.marp_bin.clone(), "node".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: crate::processors::names::MARP,
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("marp output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.marp_bin);
        if format != "html" {
            cmd.arg(format!("--{}", format));
        }
        cmd.arg("--output").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        let result = check_command_output(&out, format_args!("marp {}", input.display()));

        cleanup_marp_tmp_dirs();

        result
    }
}
```

- [ ] **Step 3: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/processor/generator/marp.rs
git commit -m "refactor: migrate MarpProcessor to ProcessorBase"
```

---

### Task 3: Migrate template processors (mako, jinja2, tera)

**Files:**
- Modify: `src/processor/generator/mako.rs`
- Modify: `src/processor/generator/jinja2.rs`
- Modify: `src/processor/generator/tera.rs`

These three share an identical `discover()` pattern using `find_templates()`. Each should use `delegate_base!(generator)` but override `auto_detect()` since they use `find_templates` instead of `scan_root_valid`.

- [ ] **Step 1: Rewrite MakoProcessor**

Replace `src/processor/generator/mako.rs`:

```rust
use anyhow::Result;
use std::process::Command;

use crate::config::{MakoConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, ProductDiscovery, clean_outputs, run_command, check_command_output};

use super::TemplateItem;

fn render_mako(item: &TemplateItem) -> Result<()> {
    crate::processors::ensure_output_dir(&item.output_path)?;

    let source = item.source_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let target = item.output_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");

    let python_script = format!(
        r#"
import mako.template, mako.lookup
lookup = mako.lookup.TemplateLookup(directories=['.'])
t = mako.template.Template(filename='{}', lookup=lookup)
output = t.render()
with open('{}', 'w') as f:
    f.write(output)
"#,
        source, target
    );

    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(&python_script);
    let output = run_command(&mut cmd)?;
    check_command_output(&output, format!("mako render {}", item.source_path.display()))
}

pub struct MakoProcessor {
    base: ProcessorBase,
    config: MakoConfig,
}

impl MakoProcessor {
    pub fn new(config: MakoConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::MAKO,
                "Render Mako templates into output files",
            ),
            config,
        }
    }
}

impl ProductDiscovery for MakoProcessor {
    delegate_base!(generator);

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        super::find_templates(&self.config.scan, file_index).is_ok_and(|t| !t.is_empty())
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let items = super::find_templates(&self.config.scan, file_index)?;
        let extra = resolve_extra_inputs(&self.config.dep_inputs)?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                crate::processors::names::MAKO,
                Some(output_config_hash(&self.config, &[])),
            )?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        render_mako(&item)
    }
}
```

- [ ] **Step 2: Rewrite Jinja2Processor (same pattern as Mako)**

Apply the same pattern to `src/processor/generator/jinja2.rs`. Read the file first, keep its `render_jinja2()` function and `execute()` logic, but replace the `impl ProductDiscovery` block with `delegate_base!(generator)` + overrides for `auto_detect`, `required_tools`, `discover`, `execute`.

- [ ] **Step 3: Rewrite TeraProcessor (same pattern, different execute)**

Apply the same pattern to `src/processor/generator/tera.rs`. Read the file first. Tera has more complex `execute()` and `discover()` logic — keep those, but use `delegate_base!(generator)` for the boilerplate and override `auto_detect`.

- [ ] **Step 4: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/processor/generator/mako.rs src/processor/generator/jinja2.rs src/processor/generator/tera.rs
git commit -m "refactor: migrate template processors to ProcessorBase"
```

---

### Task 4: Migrate remaining manual generators

**Files:**
- Modify: `src/processor/generator/pdflatex.rs`
- Modify: `src/processor/generator/pdfunite.rs`
- Modify: `src/processor/generator/rust_single_file.rs`
- Modify: `src/processor/generator/generator.rs`
- Modify: `src/processor/generator/tags.rs`
- Modify: `src/processor/generator/cc_single_file/mod.rs`
- Modify: `src/processor/generator/linux_module.rs`
- Modify: `src/processor/generator/explicit.rs`

For each processor:
1. Add `base: ProcessorBase` field to the struct
2. Initialize it in `new()` with `ProcessorBase::generator(name, description)`
3. Replace boilerplate methods with `delegate_base!(generator)`
4. Keep processor-specific overrides for `execute()`, `discover()`, `required_tools()`, and any custom methods

- [ ] **Step 1: Migrate each processor**

Read each file, apply the pattern. Keep all processor-specific logic. Only replace the boilerplate 6 methods: `description`, `processor_type`, `auto_detect`, `config_json`, `max_jobs`, `clean`.

Note: `explicit.rs` has no `max_jobs` field on its config — use the default trait method (don't include `max_jobs` in delegate, or override to return `None`). `cc_single_file` has custom `discover_for_clean` — keep that override.

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/processor/generator/
git commit -m "refactor: migrate remaining manual generators to ProcessorBase"
```

---

### Task 5: Migrate impl_generator! macro users

**Files:**
- Modify: `src/processor/generator/pandoc.rs`
- Modify: `src/processor/generator/sass.rs`
- Modify: `src/processor/generator/chromium.rs`
- Modify: `src/processor/generator/a2x.rs` (if using macro)
- Modify: `src/processor/generator/drawio.rs`
- Modify: `src/processor/generator/libreoffice.rs`
- Modify: `src/processor/generator/markdown.rs`
- Modify: `src/processor/generator/mermaid.rs`
- Modify: `src/processor/generator/objdump.rs`
- Modify: `src/processor/generator/protobuf.rs`

For each: replace `impl_generator!(...)` invocation with explicit struct + `delegate_base!(generator)` + the discover call (multi_format or single_format). These processors already have an `execute_product()` method — rename to `execute()` inside the trait impl.

- [ ] **Step 1: Migrate each `impl_generator!` user**

Example transformation for pandoc.rs:

Before:
```rust
impl_generator!(PandocProcessor, crate::config::PandocConfig,
    description: "Convert documents using pandoc",
    name: crate::processors::names::PANDOC,
    discover: multi_format, formats_field: formats,
    tool_field: pandoc
);

impl PandocProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> { ... }
}
```

After:
```rust
pub struct PandocProcessor {
    base: ProcessorBase,
    config: crate::config::PandocConfig,
}

impl PandocProcessor {
    pub fn new(config: crate::config::PandocConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::PANDOC,
                "Convert documents using pandoc",
            ),
            config,
        }
    }
}

impl ProductDiscovery for PandocProcessor {
    delegate_base!(generator);

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pandoc.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let params = super::DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: crate::processors::names::PANDOC,
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        // ... existing execute_product body ...
    }
}
```

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Remove the `impl_generator!` macro from `src/processor/generator/mod.rs`**

Delete the entire `macro_rules! impl_generator { ... }` block (lines 16-151).

- [ ] **Step 4: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/processor/generator/
git commit -m "refactor: migrate impl_generator! users to ProcessorBase, remove macro"
```

---

### Task 6: Migrate impl_checker! macro to use ProcessorBase

**Files:**
- Modify: `src/processor/checker/mod.rs`

- [ ] **Step 1: Update the `@build` variant of `impl_checker!`**

In the `@build` arm of `impl_checker!`, replace the individual boilerplate method generations with `delegate_base!(checker)` and add a `base` field to the generated struct.

The macro currently generates: `description`, `auto_detect`, `required_tools`, `discover`, `execute`, `config_json`, `batch`, `max_jobs`.

After refactor, the macro should:
1. Generate struct with `base: ProcessorBase` + `config` fields
2. Generate `new()` that initializes base
3. Use `delegate_base!(checker)` for `description`, `processor_type`, `config_json`, `max_jobs`, `auto_detect`
4. Keep macro-generated `required_tools`, `discover`, `execute`, `batch` (these have variants)

Note: `auto_detect` has 3 variants in the checker macro (scan_root, guard method, no guard). The `delegate_base!` version uses `scan_root_valid`. For guard-based variants, the macro must still override `auto_detect`.

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/processor/checker/mod.rs
git commit -m "refactor: update impl_checker! to use ProcessorBase"
```

---

### Task 7: Migrate manual checkers to ProcessorBase

**Files:**
- Modify: `src/processor/checker/zspell.rs`
- Modify: `src/processor/checker/clippy.rs`
- Modify: `src/processor/checker/make.rs`
- Modify: `src/processor/checker/aspell.rs`
- Modify: `src/processor/checker/mdl.rs`
- Modify: `src/processor/checker/markdownlint.rs`

For each: add `base: ProcessorBase` field, use `delegate_base!(checker)`, keep processor-specific methods.

- [ ] **Step 1: Migrate each manual checker**

Read each file, apply the pattern. Keep all custom `discover()`, `execute()`, `supports_batch()`, `execute_batch()` logic. Only replace: `description`, `processor_type`, `config_json`, `max_jobs`.

Note: some checkers override `auto_detect` with custom guards — keep those overrides.

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/processor/checker/
git commit -m "refactor: migrate manual checkers to ProcessorBase"
```

---

### Task 8: Migrate mass generators

**Files:**
- Modify: `src/processor/mass_generator/cargo.rs`
- Modify: `src/processor/mass_generator/cc.rs`
- Modify: `src/processor/mass_generator/gem.rs`
- Modify: `src/processor/mass_generator/jekyll.rs`
- Modify: `src/processor/mass_generator/mdbook.rs`
- Modify: `src/processor/mass_generator/npm.rs`
- Modify: `src/processor/mass_generator/pip.rs`
- Modify: `src/processor/mass_generator/sphinx.rs`

Same pattern: add `base: ProcessorBase`, use `delegate_base!(generator)`, keep custom logic.

- [ ] **Step 1: Migrate each mass generator**

Read each file, apply the pattern. Mass generators typically have custom `discover()` and `execute()` — keep those.

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/processor/mass_generator/
git commit -m "refactor: migrate mass generators to ProcessorBase"
```

---

### Task 9: Migrate LuaProcessor

**Files:**
- Modify: `src/processor/lua_processor.rs`

LuaProcessor is special — it uses dynamic `toml::Value` config, not a typed struct. It may need a custom approach or skip `delegate_base!`.

- [ ] **Step 1: Read the file and assess**

If LuaProcessor can use ProcessorBase (it has name, description, processor_type), add `base` field for those. If `config_json` and `max_jobs` don't apply (no typed config), leave those as-is.

- [ ] **Step 2: Build and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 340 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/processor/lua_processor.rs
git commit -m "refactor: migrate LuaProcessor to ProcessorBase where applicable"
```

---

### Task 10: Final cleanup and verification

**Files:**
- Verify: all test files
- Modify: `src/processor/generator/mod.rs` (remove dead macro if not done)
- Modify: `src/processor/mod.rs` (clean up any unused imports)

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All 340 tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: No warnings

- [ ] **Step 3: Build release**

Run: `cargo build --release 2>&1`
Expected: Compiles successfully

- [ ] **Step 4: Verify teaching-slides still works**

Run (from teaching-slides dir): `rsconstruct build -j 20 --dry-run 2>&1 | tail -5`
Expected: Products discovered correctly

- [ ] **Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "refactor: final cleanup after ProcessorBase migration"
```
