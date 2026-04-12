use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cli::{DisplayOptions, InputDisplay, OutputDisplay, PathFormat};
use crate::errors;
use crate::processors::names as proc_names;

/// A single build product with concrete inputs and outputs.
/// All paths are relative to project root.
#[derive(Debug, Clone)]
pub struct Product {
    /// Input files (relative paths)
    pub inputs: Vec<PathBuf>,
    /// Output files (relative paths)
    pub outputs: Vec<PathBuf>,
    /// Which processor handles this product
    pub processor: String,
    /// Unique identifier for this product
    pub id: usize,
    /// Optional hash of processor config (compiler flags, etc.)
    pub config_hash: Option<String>,
    /// Optional variant/profile name (e.g., compiler profile name)
    pub variant: Option<String>,
    /// Output directories for creators / creators (relative to project root).
    /// When non-empty, the executor caches/restores these directories instead of individual output files.
    pub output_dirs: Vec<Arc<PathBuf>>,
}

impl Product {
    pub fn new(inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, id: usize, config_hash: Option<String>) -> Self {
        Self {
            inputs,
            outputs,
            processor: processor.to_string(),
            id,
            config_hash,
            variant: None,
            output_dirs: Vec::new(),
        }
    }

    /// Create a new product with a variant/profile name
    pub fn with_variant(inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, id: usize, config_hash: Option<String>, variant: &str) -> Self {
        Self {
            inputs,
            outputs,
            processor: processor.to_string(),
            id,
            config_hash,
            variant: Some(variant.to_string()),
            output_dirs: Vec::new(),
        }
    }

    /// Return the primary (first) input file for this product.
    /// Panics if the product has no inputs (a programming error — every product must have at least one).
    pub fn primary_input(&self) -> &Path {
        self.inputs.first().expect(errors::EMPTY_PRODUCT_INPUTS)
    }

    /// Return the primary (first) output file for this product.
    /// Panics if the product has no outputs (a programming error — every generator product must have at least one).
    pub fn primary_output(&self) -> &Path {
        self.outputs.first().expect(errors::EMPTY_PRODUCT_OUTPUTS)
    }

    /// Whether this product has output directories to cache.
    pub fn has_output_dirs(&self) -> bool {
        !self.output_dirs.is_empty()
    }

    /// Compute a content-addressed descriptor key from processor identity and input content.
    /// This key does NOT include file paths — renaming a file with identical content
    /// produces the same key. The blob in the cache is path-free; the product knows
    /// where to restore it.
    pub fn descriptor_key(&self, input_checksum: &str) -> String {
        let mut parts = String::new();
        parts.push_str(&self.processor);
        if let Some(ref hash) = self.config_hash {
            parts.push(':');
            parts.push_str(hash);
        }
        if let Some(ref variant) = self.variant {
            parts.push(':');
            parts.push_str(variant);
        }
        parts.push(':');
        parts.push_str(input_checksum);
        crate::checksum::bytes_checksum(parts.as_bytes())
    }

    /// Format a path according to the given format
    fn format_path(path: &Path, format: PathFormat) -> String {
        match format {
            PathFormat::Basename => {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string()
            }
            PathFormat::Path => path.display().to_string(),
        }
    }

    /// Display name for logging with the given display options.
    /// All paths are already relative to project root.
    pub fn display(&self, opts: DisplayOptions) -> String {
        // For checkers (empty outputs), display the input file instead
        if self.outputs.is_empty() {
            return self.inputs.first()
                .map(|p| Self::format_path(p, opts.path_format))
                .unwrap_or_else(|| "?".to_string());
        }

        // Format output part
        let output_part = match opts.output {
            OutputDisplay::None => String::new(),
            OutputDisplay::Basename => {
                let names: Vec<_> = self.outputs.iter()
                    .map(|p| Self::format_path(p, PathFormat::Basename))
                    .collect();
                names.join(", ")
            }
            OutputDisplay::Path => {
                let paths: Vec<_> = self.outputs.iter()
                    .map(|p| Self::format_path(p, PathFormat::Path))
                    .collect();
                paths.join(", ")
            }
        };

        // Format input part
        let input_part = match opts.input {
            InputDisplay::None => None,
            InputDisplay::Source => {
                self.inputs.first()
                    .map(|p| Self::format_path(p, opts.path_format))
            }
            InputDisplay::All => {
                let inputs: Vec<_> = self.inputs.iter()
                    .map(|p| Self::format_path(p, opts.path_format))
                    .collect();
                if inputs.is_empty() {
                    None
                } else {
                    Some(inputs.join(", "))
                }
            }
        };

        // Combine output and input parts
        match (output_part.is_empty(), input_part) {
            (true, None) => "?".to_string(),
            (true, Some(inp)) => inp,
            (false, None) => output_part,
            (false, Some(inp)) => format!("{} <- {}", output_part, inp),
        }
    }

    /// Cache key for checksum tracking.
    /// Includes processor name, config hash, inputs, AND outputs to ensure
    /// products with the same inputs but different outputs (e.g., pandoc producing
    /// pdf, html, docx from the same source) get separate cache entries.
    pub fn cache_key(&self) -> String {
        let inputs: Vec<_> = self.inputs.iter()
            .map(|p| p.display().to_string())
            .collect();
        let outputs: Vec<_> = self.outputs.iter()
            .map(|p| p.display().to_string())
            .collect();
        let io_part = if outputs.is_empty() {
            inputs.join(":")
        } else {
            format!("{}>{}", inputs.join(":"), outputs.join(":"))
        };
        match &self.config_hash {
            Some(hash) => format!("{}:{}:{}", self.processor, hash, io_part),
            None => format!("{}:{}", self.processor, io_part),
        }
    }
}

/// Build graph with dependency resolution
#[derive(Default)]
pub struct BuildGraph {
    products: Vec<Product>,
    /// Map from output path to the single product id that produces it.
    /// One path has at most one owner by construction (output-conflict check).
    output_to_product: HashMap<PathBuf, usize>,
    /// Map from input path to every product id that consumes it.
    /// One path may feed many products (e.g. a shared header).
    input_to_products: HashMap<PathBuf, Vec<usize>>,
    /// Adjacency list: product id -> list of product ids that depend on it
    dependents: Vec<Vec<usize>>,
    /// Reverse: product id -> list of product ids it depends on
    dependencies: Vec<Vec<usize>>,
}

impl BuildGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a product to the graph.
    /// Returns an error if any output path is already claimed by another product.
    pub fn add_product(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, config_hash: Option<String>) -> Result<usize> {
        self.add_product_with_variant(inputs, outputs, processor, config_hash, None)
    }

    /// Add a product to the graph with an optional variant/profile name.
    /// Returns an error if any output path is already claimed by another product.
    pub fn add_product_with_variant(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, config_hash: Option<String>, variant: Option<&str>) -> Result<usize> {
        let id = self.products.len();

        // During fixed-point discovery, processors re-run and may re-declare
        // products that already exist. Detect and deduplicate these cases.

        // Checkers have no outputs, so the output-based dedup below won't catch them.
        // Deduplicate by matching on processor name, primary input, and variant.
        if outputs.is_empty() && !inputs.is_empty() {
            for existing in &self.products {
                if !existing.outputs.is_empty() || existing.inputs.is_empty() {
                    continue;
                }
                let same_processor = existing.processor == processor;
                if same_processor && existing.inputs[0] == inputs[0]
                    && existing.variant.as_deref() == variant
                {
                    return Ok(existing.id);
                }
            }
        }

        // For generators: check output conflicts and deduplicate re-declarations.
        for output in &outputs {
            if let Some(&existing_id) = self.output_to_product.get(output) {
                let existing = self.products.get(existing_id).expect(crate::errors::INVALID_PRODUCT_ID);
                let same_processor = existing.processor == processor;
                // Same processor re-declaring the same outputs: update inputs if
                // they grew (virtual files from upstream generators were added).
                let is_superset = same_processor && existing.outputs == outputs
                    && existing.inputs.iter().all(|i| inputs.contains(i));
                if is_superset {
                    if existing.inputs != inputs {
                        // Remove stale index entries for any old inputs that disappeared
                        // (shouldn't happen with superset, but keep the index honest).
                        let old_inputs = std::mem::replace(&mut self.products[existing_id].inputs, inputs.clone());
                        for old in &old_inputs {
                            if !inputs.contains(old) {
                                if let Some(ids) = self.input_to_products.get_mut(old) {
                                    ids.retain(|&x| x != existing_id);
                                }
                            }
                        }
                        // Add index entries for the new inputs that weren't there before.
                        for new in &inputs {
                            if !old_inputs.contains(new) {
                                self.input_to_products.entry(new.clone()).or_default().push(existing_id);
                            }
                        }
                    }
                    return Ok(existing_id);
                }
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::GraphError,
                    format!(
                        "Output conflict: {} is produced by both [{}] and [{}]",
                        output.display(),
                        existing.processor,
                        processor,
                    ),
                ).into());
            }
        }

        // Register outputs before moving outputs into the product
        for output in &outputs {
            self.output_to_product.insert(output.clone(), id);
        }

        // Register inputs in the input index
        for input in &inputs {
            self.input_to_products.entry(input.clone()).or_default().push(id);
        }

        let product = match variant {
            Some(v) => Product::with_variant(inputs, outputs, processor, id, config_hash, v),
            None => Product::new(inputs, outputs, processor, id, config_hash),
        };

        self.products.push(product);
        self.dependents.push(Vec::new());
        self.dependencies.push(Vec::new());

        Ok(id)
    }

    /// Add a product with an output directory for creator caching.
    /// The output_dir is the directory whose contents will be cached/restored as a whole.
    pub fn add_product_with_output_dir(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, config_hash: Option<String>, output_dir: PathBuf) -> Result<usize> {
        self.add_product_with_output_dirs_and_variant(inputs, outputs, processor, config_hash, vec![output_dir], None)
    }

    /// Add a product with an output directory and an optional variant/profile name.
    pub fn add_product_with_output_dir_and_variant(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, config_hash: Option<String>, output_dir: PathBuf, variant: Option<&str>) -> Result<usize> {
        self.add_product_with_output_dirs_and_variant(inputs, outputs, processor, config_hash, vec![output_dir], variant)
    }

    /// Add a product with multiple output directories and an optional variant/profile name.
    pub fn add_product_with_output_dirs_and_variant(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, config_hash: Option<String>, output_dirs: Vec<PathBuf>, variant: Option<&str>) -> Result<usize> {
        let id = self.add_product_with_variant(inputs, outputs, processor, config_hash, variant)?;
        self.products[id].output_dirs = output_dirs.into_iter().map(Arc::new).collect();
        Ok(id)
    }

    /// Incorporate tool version hashes into product config hashes.
    /// For each product whose processor has an entry in the map, the tool
    /// version hash is appended to (or becomes) the product's config_hash.
    pub fn apply_tool_version_hashes(&mut self, processor_tool_hashes: &HashMap<String, String>) {
        for product in &mut self.products {
            if let Some(tool_hash) = processor_tool_hashes.get(&product.processor) {
                product.config_hash = Some(match &product.config_hash {
                    Some(existing) => format!("{}:{}", existing, tool_hash),
                    None => tool_hash.clone(),
                });
            }
        }
    }

    /// Resolve dependencies between products
    pub fn resolve_dependencies(&mut self) {
        // Collect edges first to avoid borrow conflict with self.products
        let edges: Vec<(usize, usize)> = self.products.iter()
            .flat_map(|product| {
                product.inputs.iter().filter_map(|input| {
                    self.output_to_product.get(input)
                        .copied()
                        .filter(|&producer_id| producer_id != product.id)
                        .map(|producer_id| (producer_id, product.id))
                })
            })
            .collect();

        for (producer_id, consumer_id) in edges {
            self.dependents.get_mut(producer_id).expect(crate::errors::INVALID_PRODUCT_ID).push(consumer_id);
            self.dependencies.get_mut(consumer_id).expect(crate::errors::INVALID_PRODUCT_ID).push(producer_id);
        }
    }

    /// Topological sort - returns product ids in execution order
    /// Returns error if there's a cycle
    pub fn topological_sort(&self) -> Result<Vec<usize>> {
        let mut in_degree: Vec<usize> = self.dependencies.iter()
            .map(|deps| deps.len())
            .collect();

        // Start with products that have no dependencies (BTreeSet keeps sorted order)
        let mut queue: BTreeSet<usize> = in_degree.iter()
            .enumerate()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(id, _)| id)
            .collect();

        let mut result = Vec::with_capacity(self.products.len());

        while let Some(id) = queue.pop_first() {
            result.push(id);

            // Reduce in-degree of dependents
            for &dep_id in self.dependents.get(id).expect(crate::errors::INVALID_PRODUCT_ID) {
                in_degree[dep_id] = in_degree[dep_id].saturating_sub(1);
                if in_degree[dep_id] == 0 {
                    queue.insert(dep_id);
                }
            }
        }

        if result.len() != self.products.len() {
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::GraphError,
                "Cycle detected in build graph",
            ).into());
        }

        Ok(result)
    }

    /// Get a product by id
    pub fn get_product(&self, id: usize) -> Option<&Product> {
        self.products.get(id)
    }

    /// Get all products
    pub fn products(&self) -> &[Product] {
        &self.products
    }

    /// Return the id of the product that declares `path` as one of its outputs,
    /// or None if no product owns it. O(1) average — backed by a hashmap index.
    ///
    /// Used by Creators caching a shared output directory: any path owned by a
    /// different product must be excluded from this Creator's tree so restore
    /// never clobbers another processor's file.
    pub fn path_owner(&self, path: &Path) -> Option<usize> {
        self.output_to_product.get(path).copied()
    }

    /// Return every product id that lists `path` as an input. O(1) average — backed
    /// by a hashmap index. Returns an empty slice if the path is not an input to
    /// any product.
    pub fn products_consuming(&self, path: &Path) -> &[usize] {
        self.input_to_products.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Get dependencies of a product (products that must be built before this one)
    pub fn get_dependencies(&self, id: usize) -> &[usize] {
        self.dependencies.get(id).expect(crate::errors::INVALID_PRODUCT_ID)
    }

    /// Get processor-level dependencies: returns a map from processor name
    /// to the set of processor names it depends on.
    pub fn processor_dependencies(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for product in &self.products {
            deps.entry(product.processor.clone()).or_default();
            for &dep_id in self.dependencies.get(product.id).expect(crate::errors::INVALID_PRODUCT_ID) {
                let dep_proc = &self.products[dep_id].processor;
                if dep_proc != &product.processor {
                    deps.entry(product.processor.clone()).or_default().insert(dep_proc.clone());
                }
            }
        }
        deps
    }

    /// Get mutable access to a product by id
    pub fn get_product_mut(&mut self, id: usize) -> Option<&mut Product> {
        self.products.get_mut(id)
    }

    /// Filter the graph to only include products whose input files match any of the target patterns.
    /// Uses glob matching. Products not matching any pattern are removed.
    pub fn filter_by_targets(&mut self, patterns: &[String]) {
        let compiled: Vec<glob::Pattern> = patterns.iter()
            .filter_map(|p| glob::Pattern::new(p).ok())
            .collect();
        if compiled.is_empty() {
            return;
        }

        // Collect IDs to keep
        let keep: HashSet<usize> = self.products.iter()
            .filter(|product| {
                product.inputs.iter().any(|input| {
                    let input_str = input.display().to_string();
                    compiled.iter().any(|pat| pat.matches(&input_str))
                })
            })
            .map(|p| p.id)
            .collect();

        // Remove products that don't match (clear their inputs/outputs so they become no-ops)
        // We can't actually remove elements because IDs are indices, so we rebuild the graph
        let old_products = std::mem::take(&mut self.products);
        self.output_to_product.clear();
        self.input_to_products.clear();
        self.dependents.clear();
        self.dependencies.clear();

        for product in old_products {
            if keep.contains(&product.id) {
                let id = self.products.len();
                for output in &product.outputs {
                    self.output_to_product.insert(output.clone(), id);
                }
                for input in &product.inputs {
                    self.input_to_products.entry(input.clone()).or_default().push(id);
                }
                let mut p = product;
                p.id = id;
                self.products.push(p);
                self.dependents.push(Vec::new());
                self.dependencies.push(Vec::new());
            }
        }
    }
}

impl BuildGraph {
    /// Generate a safe node ID from a path
    fn path_node_id(path: &Path) -> String {
        let s = path.display().to_string();
        // Make safe for DOT/Mermaid: replace special chars
        format!("f_{}", s.replace(['.', '-', '/', ' '], "_"))
    }

    /// Generate a node ID for a processor
    fn processor_node_id(product: &Product) -> String {
        format!("proc_{}", product.id)
    }

    /// Get file label (just the filename)
    fn file_label(path: &Path) -> String {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    /// Format graph as DOT (Graphviz)
    pub fn to_dot(&self) -> String {
        let mut buf = String::new();
        let _ = writeln!(buf, "digraph build_graph {{");
        let _ = writeln!(buf, "    rankdir=LR;");

        // Collect all unique input and output files
        let mut input_files: HashSet<PathBuf> = HashSet::new();
        let mut output_files: HashSet<PathBuf> = HashSet::new();

        for product in &self.products {
            for input in &product.inputs {
                input_files.insert(input.clone());
            }
            for output in &product.outputs {
                output_files.insert(output.clone());
            }
        }

        // Add file nodes (inputs that are not outputs = source files)
        let _ = writeln!(buf, "\n    // Source files");
        for file in &input_files {
            if !output_files.contains(file) {
                let node_id = Self::path_node_id(file);
                let label = Self::file_label(file);
                let _ = writeln!(buf, "    {} [label=\"{}\" shape=note style=filled fillcolor=white];", node_id, label);
            }
        }

        let _ = writeln!(buf, "\n    // Generated files");
        for file in &output_files {
            let node_id = Self::path_node_id(file);
            let label = Self::file_label(file);
            let color = if input_files.contains(file) { "lightgreen" } else { "lightyellow" };
            let _ = writeln!(buf, "    {} [label=\"{}\" shape=note style=filled fillcolor={}];", node_id, label, color);
        }

        let _ = writeln!(buf, "\n    // Processors");
        for product in &self.products {
            let node_id = Self::processor_node_id(product);
            let color = match product.processor.as_str() {
                proc_names::TERA => "lightblue",
                proc_names::CC_SINGLE_FILE => "lightsalmon",
                _ => "lightgray",
            };
            let _ = writeln!(buf, "    {} [label=\"{}\" shape=box style=filled fillcolor={}];",
                node_id, product.processor, color);
        }

        let _ = writeln!(buf, "\n    // Edges");
        for product in &self.products {
            let proc_id = Self::processor_node_id(product);

            // Input files -> processor
            for input in &product.inputs {
                let input_id = Self::path_node_id(input);
                let _ = writeln!(buf, "    {} -> {};", input_id, proc_id);
            }

            // Processor -> output files
            for output in &product.outputs {
                let output_id = Self::path_node_id(output);
                let _ = writeln!(buf, "    {} -> {};", proc_id, output_id);
            }
        }

        let _ = write!(buf, "}}");
        buf
    }

    /// Format graph as Mermaid
    /// Only shows primary source files (first input per product), not headers,
    /// to keep the diagram manageable for large projects.
    pub fn to_mermaid(&self) -> String {
        let mut buf = String::new();
        let _ = writeln!(buf, "graph LR");

        // Collect primary source files (first input only) and output files
        let mut source_files: HashSet<PathBuf> = HashSet::new();
        let mut output_files: HashSet<PathBuf> = HashSet::new();

        for product in &self.products {
            if let Some(first_input) = product.inputs.first() {
                source_files.insert(first_input.clone());
            }
            for output in &product.outputs {
                output_files.insert(output.clone());
            }
        }

        let _ = writeln!(buf, "\n    %% Source files");
        for file in &source_files {
            if !output_files.contains(file) {
                let node_id = Self::path_node_id(file);
                let label = Self::file_label(file);
                let _ = writeln!(buf, "    {}[/\"{}\"/]", node_id, label);
            }
        }

        let _ = writeln!(buf, "\n    %% Generated files");
        for file in &output_files {
            let node_id = Self::path_node_id(file);
            let label = Self::file_label(file);
            let _ = writeln!(buf, "    {}[/\"{}\"/]", node_id, label);
        }

        let _ = writeln!(buf, "\n    %% Processors");
        for product in &self.products {
            let node_id = Self::processor_node_id(product);
            let _ = writeln!(buf, "    {}[\"{}\" ]", node_id, product.processor);
        }

        let _ = writeln!(buf, "\n    %% Edges");
        for product in &self.products {
            let proc_id = Self::processor_node_id(product);

            // Only connect primary source file (first input), skip headers
            if let Some(first_input) = product.inputs.first() {
                let input_id = Self::path_node_id(first_input);
                let _ = writeln!(buf, "    {} --> {}", input_id, proc_id);
            }

            for output in &product.outputs {
                let output_id = Self::path_node_id(output);
                let _ = writeln!(buf, "    {} --> {}", proc_id, output_id);
            }
        }

        // Add styling
        let tera_procs: Vec<_> = self.products.iter()
            .filter(|p| p.processor == proc_names::TERA)
            .map(Self::processor_node_id)
            .collect();
        let cc_procs: Vec<_> = self.products.iter()
            .filter(|p| p.processor == proc_names::CC_SINGLE_FILE)
            .map(Self::processor_node_id)
            .collect();

        for proc_id in &tera_procs {
            let _ = writeln!(buf, "\n    style {} fill:#add8e6", proc_id);
        }
        for proc_id in &cc_procs {
            let _ = writeln!(buf, "\n    style {} fill:#ffa07a", proc_id);
        }

        buf.truncate(buf.trim_end().len());
        buf
    }

    /// Format graph as JSON
    pub fn to_json(&self) -> String {
        let nodes: Vec<serde_json::Value> = self.products.iter()
            .map(|product| {
                let inputs: Vec<String> = product.inputs.iter()
                    .map(|p| p.display().to_string())
                    .collect();
                let outputs: Vec<String> = product.outputs.iter()
                    .map(|p| p.display().to_string())
                    .collect();
                serde_json::json!({
                    "id": product.id,
                    "processor": product.processor,
                    "inputs": inputs,
                    "outputs": outputs,
                    "depends_on": self.dependencies.get(product.id).expect(errors::INVALID_PRODUCT_ID),
                })
            })
            .collect();

        let root = serde_json::json!({ "products": nodes });
        serde_json::to_string_pretty(&root).expect(errors::JSON_SERIALIZE)
    }

    /// Format graph as plain text
    pub fn to_text(&self) -> String {
        let mut buf = String::new();
        let _ = writeln!(buf, "Build Dependency Graph");
        let _ = writeln!(buf, "======================");

        // Get topological order
        let order = match self.topological_sort() {
            Ok(o) => o,
            Err(_) => {
                let _ = writeln!(buf, "Error: Cycle detected in graph");
                buf.truncate(buf.trim_end().len());
                return buf;
            }
        };

        for id in order {
            let product = self.products.get(id).expect(errors::INVALID_PRODUCT_ID);
            let inputs: Vec<_> = product.inputs.iter()
                .filter_map(|p| p.file_name())
                .filter_map(|n| n.to_str())
                .collect();
            let outputs: Vec<_> = product.outputs.iter()
                .filter_map(|p| p.file_name())
                .filter_map(|n| n.to_str())
                .collect();

            let _ = writeln!(buf, "[{}] {} -> {}",
                product.processor,
                inputs.join(", "),
                outputs.join(", "));

            // Show dependencies
            let deps = self.dependencies.get(product.id).expect(errors::INVALID_PRODUCT_ID);
            if !deps.is_empty() {
                let dep_names: Vec<_> = deps.iter()
                    .map(|&d| {
                        let dep = self.products.get(d).expect(errors::INVALID_PRODUCT_ID);
                        let out: Vec<_> = dep.outputs.iter()
                            .filter_map(|p| p.file_name())
                            .filter_map(|n| n.to_str())
                            .collect();
                        out.join(", ")
                    })
                    .collect();
                let _ = writeln!(buf, "    depends on: {}", dep_names.join(", "));
            }
        }

        if self.products.is_empty() {
            let _ = writeln!(buf, "(empty graph)");
        }

        buf.truncate(buf.trim_end().len());
        buf
    }

    /// Generate SVG by piping DOT through the `dot` command
    pub fn to_svg(&self) -> Result<String> {
        use std::process::{Command, Stdio};
        use std::io::Write;
        use crate::processors::{check_command_output, log_command};

        let dot_content = self.to_dot();

        let mut cmd = Command::new("dot");
        cmd.arg("-Tsvg")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        log_command(&cmd);
        let mut child = cmd
            .spawn()
            .map_err(|_| anyhow::anyhow!("Graphviz 'dot' command not found. Install Graphviz to use SVG format"))?;

        child.stdin.take().expect(errors::STDIN_PIPED).write_all(dot_content.as_bytes())?;

        let output = child.wait_with_output()?;
        check_command_output(&output, "dot")?;

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Generate a self-contained HTML file with Mermaid diagram
    pub fn to_html(&self) -> String {
        let mermaid_content = self.to_mermaid();
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>RSConstruct Build Graph</title>
    <script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 40px;
            background: #f5f5f5;
        }}
        h1 {{
            color: #333;
        }}
        .mermaid {{
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
    </style>
</head>
<body>
    <h1>RSConstruct Build Graph</h1>
    <div class="mermaid">
{mermaid_content}
    </div>
    <script>
        mermaid.initialize({{ startOnLoad: true, theme: 'default', maxTextSize: 500000 }});
    </script>
</body>
</html>
"#)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_product_assigns_incrementing_ids() {
        let mut g = BuildGraph::new();
        let id0 = g.add_product(vec!["a.c".into()], vec!["a.o".into()], "cc", None).unwrap();
        let id1 = g.add_product(vec!["b.c".into()], vec!["b.o".into()], "cc", None).unwrap();
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(g.products().len(), 2);
    }

    #[test]
    fn output_conflict_is_detected() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["a.c".into()], vec!["out.o".into()], "cc", None).unwrap();
        let result = g.add_product(vec!["b.c".into()], vec!["out.o".into()], "cc", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Output conflict"));
    }

    #[test]
    fn topological_sort_no_dependencies() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["c.c".into()], vec![], "check", None).unwrap();
        g.add_product(vec!["b.c".into()], vec![], "check", None).unwrap();
        g.add_product(vec!["a.c".into()], vec![], "check", None).unwrap();
        g.resolve_dependencies();
        let order = g.topological_sort().unwrap();
        // All products have no dependencies, order should contain all ids
        assert_eq!(order.len(), 3);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2]);
    }

    #[test]
    fn topological_sort_respects_dependencies() {
        let mut g = BuildGraph::new();
        // Product 0: generates lib.o
        g.add_product(vec!["lib.c".into()], vec!["lib.o".into()], "cc", None).unwrap();
        // Product 1: consumes lib.o (depends on product 0)
        g.add_product(vec!["main.c".into(), "lib.o".into()], vec!["main".into()], "cc", None).unwrap();
        g.resolve_dependencies();
        let order = g.topological_sort().unwrap();
        assert_eq!(order.len(), 2);
        // lib.o producer (0) must come before consumer (1)
        let pos0 = order.iter().position(|&id| id == 0).unwrap();
        let pos1 = order.iter().position(|&id| id == 1).unwrap();
        assert!(pos0 < pos1);
    }

    #[test]
    fn topological_sort_chain() {
        let mut g = BuildGraph::new();
        // A -> B -> C chain
        g.add_product(vec!["a.c".into()], vec!["a.o".into()], "cc", None).unwrap();
        g.add_product(vec!["a.o".into()], vec!["b.o".into()], "link", None).unwrap();
        g.add_product(vec!["b.o".into()], vec!["c.out".into()], "link", None).unwrap();
        g.resolve_dependencies();
        let order = g.topological_sort().unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn cycle_detection() {
        let mut g = BuildGraph::new();
        // Create a cycle: 0 produces a.o, 1 produces b.o, but each consumes the other
        g.add_product(vec!["b.o".into()], vec!["a.o".into()], "cc", None).unwrap();
        g.add_product(vec!["a.o".into()], vec!["b.o".into()], "cc", None).unwrap();
        g.resolve_dependencies();
        let result = g.topological_sort();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cycle"));
    }

    #[test]
    fn resolve_dependencies_links_products() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["src.c".into()], vec!["obj.o".into()], "cc", None).unwrap();
        g.add_product(vec!["obj.o".into()], vec!["app".into()], "link", None).unwrap();
        g.resolve_dependencies();
        // Product 1 depends on product 0
        assert_eq!(g.get_dependencies(1), &[0]);
        // Product 0 has no dependencies
        assert!(g.get_dependencies(0).is_empty());
    }

    #[test]
    fn cache_key_differs_for_different_outputs() {
        // Regression test: products with same inputs but different outputs
        // (e.g., pandoc producing pdf, html, docx from the same .md file)
        // must have different cache keys. Otherwise they overwrite each other's
        // cache entries and cause stale output bugs.
        let p_pdf = Product::new(
            vec!["doc.md".into()], vec!["out/doc.pdf".into()], "pandoc", 0, Some("h".into()));
        let p_html = Product::new(
            vec!["doc.md".into()], vec!["out/doc.html".into()], "pandoc", 0, Some("h".into()));
        let p_docx = Product::new(
            vec!["doc.md".into()], vec!["out/doc.docx".into()], "pandoc", 0, Some("h".into()));

        assert_ne!(p_pdf.cache_key(), p_html.cache_key(),
            "PDF and HTML products must have different cache keys");
        assert_ne!(p_html.cache_key(), p_docx.cache_key(),
            "HTML and DOCX products must have different cache keys");
        assert_ne!(p_pdf.cache_key(), p_docx.cache_key(),
            "PDF and DOCX products must have different cache keys");
    }

    #[test]
    fn cache_key_includes_config_hash() {
        let p1 = Product::new(vec!["a.c".into()], vec![], "cc", 0, None);
        let p2 = Product::new(vec!["a.c".into()], vec![], "cc", 0, Some("abc123".into()));
        assert!(!p1.cache_key().contains("abc123"));
        assert!(p2.cache_key().contains("abc123"));
    }

    #[test]
    fn apply_tool_version_hashes() {
        let mut g = BuildGraph::new();
        g.add_product(vec!["a.c".into()], vec![], "cc", Some("cfg1".into())).unwrap();
        g.add_product(vec!["b.py".into()], vec![], "ruff", None).unwrap();
        let mut hashes = HashMap::new();
        hashes.insert("cc".into(), "toolv1".into());
        g.apply_tool_version_hashes(&hashes);
        // cc product gets tool hash appended
        assert!(g.get_product(0).unwrap().config_hash.as_ref().unwrap().contains("toolv1"));
        assert!(g.get_product(0).unwrap().config_hash.as_ref().unwrap().contains("cfg1"));
        // ruff product (no tool hash mapping) stays None
        assert!(g.get_product(1).unwrap().config_hash.is_none());
    }

    #[test]
    fn empty_graph_sorts_ok() {
        let g = BuildGraph::new();
        let order = g.topological_sort().unwrap();
        assert!(order.is_empty());
    }
}
