# Instance-Aware Product Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix multi-instance processors so each instance discovers its own products independently, instead of the second instance being silently deduplicated away by the graph's prefix-matching logic.

**Architecture:** Add an `instance_name` field to every processor struct. The `ProductDiscovery::discover()` method signature gains an `instance_name: &str` parameter. The builder passes the instance name (e.g., `"script.md_lint"`) to `discover()`, which flows through to `graph.add_product()`. This eliminates the need for `remap_processor_name` entirely — products get the correct processor name from the start. The `same_processor` dedup in `BuildGraph::add_product_with_variant` becomes an exact match only.

**Tech Stack:** Rust, macros (`impl_checker!`, `delegate_base!`, `for_each_processor!`)

---

### Task 1: Write the failing test

**Files:**
- Modify: `tests/processor/script.rs`

- [ ] **Step 1: Write the failing test**

Add a test that creates two multi-instance script processors with identical scan config and verifies both discover files:

```rust
#[test]
fn script_multi_instance_both_discover_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::write(
        project_path.join("rsconstruct.toml"),
        concat!(
            "[processor.script.lint_a]\n",
            "linter = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
            "\n",
            "[processor.script.lint_b]\n",
            "linter = \"true\"\n",
            "src_extensions = [\".txt\"]\n",
        ),
    )
    .unwrap();

    fs::write(project_path.join("test.txt"), "hello\n").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build should succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[script.lint_a]"),
        "Should process script.lint_a: {}",
        stdout
    );
    assert!(
        stdout.contains("[script.lint_b]"),
        "Should process script.lint_b: {}",
        stdout
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test script_multi_instance_both_discover_files -- --nocapture 2>&1`
Expected: FAIL — `script.lint_b` will not appear in stdout because its products are deduplicated away.

- [ ] **Step 3: Commit**

```bash
git add tests/processor/script.rs
git commit -m "test: add failing test for multi-instance script discovery bug"
```

---

### Task 2: Add `instance_name` parameter to `ProductDiscovery::discover()`

**Files:**
- Modify: `src/processor/mod.rs` (trait definition, doc examples, `discover_checker_products`, `discover_for_clean` default)

- [ ] **Step 1: Change the trait signature**

In `src/processor/mod.rs`, change:

```rust
fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()>;
```

to:

```rust
fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()>;
```

Also change the `discover_for_clean` default implementation:

```rust
fn discover_for_clean(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
    self.discover(graph, file_index, instance_name)
}
```

Update the doc examples in the trait documentation to include the new parameter.

- [ ] **Step 2: Update `discover_checker_products` to not take a hardcoded name**

The `discover_checker_products` function already takes `processor_name: &str` — no signature change needed. The callers will change to pass `instance_name` instead of the type constant.

- [ ] **Step 3: Compile to see all errors**

Run: `cargo build 2>&1 | head -80`
Expected: Compilation errors in every file that implements `ProductDiscovery::discover()`. This is the list of files to fix in the following tasks.

---

### Task 3: Update `impl_checker!` macro to pass `instance_name` through

**Files:**
- Modify: `src/processor/checker/mod.rs`

- [ ] **Step 1: Update the generated `discover` and `discover_for_clean` methods**

In the `impl_checker!` macro, change the generated `discover` method from:

```rust
fn discover(
    &self,
    graph: &mut $crate::graph::BuildGraph,
    file_index: &$crate::file_index::FileIndex,
) -> anyhow::Result<()> {
    impl_checker!(@discover self, graph, file_index, $config_field, $name, [$($guard)?])
}
```

to:

```rust
fn discover(
    &self,
    graph: &mut $crate::graph::BuildGraph,
    file_index: &$crate::file_index::FileIndex,
    instance_name: &str,
) -> anyhow::Result<()> {
    impl_checker!(@discover self, graph, file_index, $config_field, instance_name, [$($guard)?])
}
```

Note: `$name` (the type constant) is replaced with `instance_name` (the runtime parameter). The `$name` field in the macro is still used for `description()` etc., but `discover` now uses the passed-in instance name.

- [ ] **Step 2: Compile to check macro expansion is correct**

Run: `cargo build 2>&1 | head -40`
Expected: Errors only in non-macro processor implementations (the manual `discover` methods), not in macro-generated ones.

---

### Task 4: Update `delegate_base!` macro for generators

**Files:**
- Modify: `src/processor/mod.rs` (the `delegate_base!` macro)

- [ ] **Step 1: Check which `delegate_base!` variants generate `discover` or `discover_for_clean`**

The `delegate_base!` macro generates common trait methods. Check if it generates `discover` or `discover_for_clean`. If it does, update those to include `instance_name: &str`.

Based on the codebase, `delegate_base!` does NOT generate `discover` — it generates `description`, `auto_detect`, `config_json`, `max_jobs`, and `clean`. So no changes needed here.

- [ ] **Step 2: Verify**

Run: `cargo build 2>&1 | grep "delegate_base"` — should produce no errors.

---

### Task 5: Update all manual `discover()` implementations in checkers

**Files:**
- Modify: `src/processor/checker/terms.rs`
- Modify: `src/processor/checker/aspell.rs`
- Modify: `src/processor/checker/zspell.rs`
- Modify: `src/processor/checker/mdl.rs`
- Modify: `src/processor/checker/markdownlint.rs`
- Modify: `src/processor/checker/clippy.rs`
- Modify: `src/processor/checker/make.rs`

For each file that manually implements `discover()` (not through `impl_checker!`):

- [ ] **Step 1: Add `instance_name: &str` parameter to each `discover` signature**

Change each from:
```rust
fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
```
to:
```rust
fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
```

Replace the hardcoded `crate::processors::names::*` constant in the `discover_checker_products(...)` or `graph.add_product(...)` call with `instance_name`.

- [ ] **Step 2: Update `discover_for_clean` overrides if any exist in these files**

Search for `discover_for_clean` in these files and add the `instance_name` parameter, passing it through.

- [ ] **Step 3: Compile**

Run: `cargo build 2>&1 | head -40`

---

### Task 6: Update all generator `discover()` implementations

**Files:**
- Modify: `src/processor/generator/generator.rs`
- Modify: `src/processor/generator/explicit.rs`
- Modify: `src/processor/generator/pdfunite.rs`
- Modify: `src/processor/generator/rust_single_file.rs`
- Modify: `src/processor/generator/jinja2.rs`
- Modify: `src/processor/generator/tera.rs`
- Modify: `src/processor/generator/mako.rs`
- Modify: `src/processor/generator/tags.rs`
- Modify: `src/processor/generator/linux_module.rs`
- Modify: `src/processor/generator/cc_single_file/mod.rs`
- Modify: `src/processor/generator/protobuf.rs`
- Modify: `src/processor/generator/pdflatex.rs`
- Modify: `src/processor/generator/pandoc.rs`
- Modify: `src/processor/generator/sass.rs`
- Modify: `src/processor/generator/mermaid.rs`
- Modify: `src/processor/generator/a2x.rs`
- Modify: `src/processor/generator/chromium.rs`
- Modify: `src/processor/generator/objdump.rs`
- Modify: `src/processor/generator/drawio.rs`
- Modify: `src/processor/generator/markdown.rs`
- Modify: `src/processor/generator/libreoffice.rs`
- Modify: `src/processor/generator/marp.rs`

For each file:

- [ ] **Step 1: Add `instance_name: &str` to `discover` and `discover_for_clean` signatures**

Replace `crate::processors::names::*` constant in `graph.add_product(...)` calls and `DiscoverParams` structs with `instance_name`.

- [ ] **Step 2: Compile**

Run: `cargo build 2>&1 | head -40`

---

### Task 7: Update mass generator `discover()` implementations

**Files:**
- Modify: `src/processor/mass_generator/sphinx.rs`
- Modify: `src/processor/mass_generator/pip.rs`
- Modify: `src/processor/mass_generator/npm.rs`
- Modify: `src/processor/mass_generator/mdbook.rs`
- Modify: `src/processor/mass_generator/jekyll.rs`
- Modify: `src/processor/mass_generator/gem.rs`
- Modify: `src/processor/mass_generator/cc.rs`
- Modify: `src/processor/mass_generator/cargo.rs`

Same pattern as Task 6: add `instance_name: &str` parameter, pass it through to `graph.add_product*()` and `DirectoryProductOpts`.

- [ ] **Step 1: Update all mass generator discover methods**

- [ ] **Step 2: Update `DirectoryProductOpts` if it stores processor name**

Check if `DirectoryProductOpts` has a `processor_name` field. If so, callers should pass `instance_name` instead of the constant.

- [ ] **Step 3: Compile**

Run: `cargo build 2>&1 | head -40`

---

### Task 8: Update `LuaProcessor::discover()`

**Files:**
- Modify: `src/processor/lua_processor.rs`

- [ ] **Step 1: Add `instance_name: &str` to discover signature**

`LuaProcessor` already uses `&self.name` which is the instance name. Change the signature to accept `instance_name` but continue using `&self.name` (they should be the same).

- [ ] **Step 2: Compile**

Run: `cargo build 2>&1 | head -40`

---

### Task 9: Update the builder to pass instance names and remove `remap_processor_name`

**Files:**
- Modify: `src/builder/mod.rs`

- [ ] **Step 1: Pass instance name to `discover()` and `discover_for_clean()`**

In `discover_products()`, change:

```rust
processors[name].discover(graph, &file_index)?;
```
to:
```rust
processors[name].discover(graph, &file_index, name)?;
```

And same for `discover_for_clean`:
```rust
processors[name].discover_for_clean(graph, &file_index, name)?;
```

- [ ] **Step 2: Remove the `remap_processor_name` call**

Delete these lines from `discover_products()`:
```rust
if let Some(type_name) = instance_to_type.get(name) {
    graph.remap_processor_name(type_name, name);
}
```

Also remove the `instance_to_type_map()` method and the `instance_to_type` variable if no longer used.

- [ ] **Step 3: Compile**

Run: `cargo build 2>&1 | head -20`

---

### Task 10: Remove `remap_processor_name` from `BuildGraph`

**Files:**
- Modify: `src/graph.rs`

- [ ] **Step 1: Delete the `remap_processor_name` method**

Remove:
```rust
pub fn remap_processor_name(&mut self, old_name: &str, new_name: &str) {
    for product in &mut self.products {
        if product.processor == old_name {
            product.processor = new_name.to_string();
        }
    }
}
```

- [ ] **Step 2: Remove the prefix-matching dedup from `add_product_with_variant`**

Change the `same_processor` check from:
```rust
let same_processor = existing.processor == processor
    || existing.processor.starts_with(&format!("{}.", processor))
    || processor.starts_with(&format!("{}.", existing.processor));
```
to exact match only:
```rust
let same_processor = existing.processor == processor;
```

Do this for BOTH the checker dedup block (empty outputs) and the generator dedup block (output conflicts).

- [ ] **Step 3: Compile and run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass, including the new `script_multi_instance_both_discover_files` test.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "fix: pass instance names through discover() to fix multi-instance dedup bug

Multi-instance processors (e.g., script.lint_a and script.lint_b) were
silently deduplicated because discover() used the type name ('script')
and add_product used prefix matching to consider them the same processor.

Now discover() receives the instance name directly, products get the
correct processor name from the start, and remap_processor_name is
removed entirely."
```

---

### Task 11: Update `clean_outputs` callers to use `instance_name`

**Files:**
- All generators that call `clean_outputs(product, "processor_name", verbose)`

- [ ] **Step 1: Check if `clean()` has access to the instance name**

The `clean()` method on `ProductDiscovery` receives a `&Product` which has `product.processor` — this is the instance name (now correct since discover sets it). So generators can use `&product.processor` instead of a hardcoded constant for the label.

Change all calls like:
```rust
fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
    clean_outputs(product, "mygen", verbose)
}
```
to:
```rust
fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
    clean_outputs(product, &product.processor, verbose)
}
```

This is optional (it's only a display label) but makes the output correct for multi-instance generators.

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: use product.processor for clean_outputs label"
```

---

### Task 12: Verify with the teaching-slides project

- [ ] **Step 1: Run status in teaching-slides**

Run: `cd ../teaching-slides && rsconstruct status`
Expected: Both `script.md_lint` and `script.check_code_labels` show 763 files (or similar non-zero count).

- [ ] **Step 2: Run the full rsconstruct test suite one final time**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 3: Final commit if any fixups needed**
