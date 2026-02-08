use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cli::{DisplayOptions, InputDisplay, OutputDisplay, PathFormat};

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
        }
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

    /// Cache key for checksum tracking
    pub fn cache_key(&self) -> String {
        let inputs: Vec<_> = self.inputs.iter()
            .map(|p| p.display().to_string())
            .collect();
        match &self.config_hash {
            Some(hash) => format!("{}:{}:{}", self.processor, hash, inputs.join(":")),
            None => format!("{}:{}", self.processor, inputs.join(":")),
        }
    }
}

/// Build graph with dependency resolution
pub struct BuildGraph {
    products: Vec<Product>,
    /// Map from output path to product id
    output_to_product: HashMap<PathBuf, usize>,
    /// Adjacency list: product id -> list of product ids that depend on it
    dependents: Vec<Vec<usize>>,
    /// Reverse: product id -> list of product ids it depends on
    dependencies: Vec<Vec<usize>>,
}

impl BuildGraph {
    pub fn new() -> Self {
        Self {
            products: Vec::new(),
            output_to_product: HashMap::new(),
            dependents: Vec::new(),
            dependencies: Vec::new(),
        }
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

        // Check for output conflicts before mutating anything
        for output in &outputs {
            if let Some(&existing_id) = self.output_to_product.get(output) {
                let existing = &self.products[existing_id];
                return Err(crate::exit_code::RsbError::new(
                    crate::exit_code::RsbExitCode::GraphError,
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

        let product = match variant {
            Some(v) => Product::with_variant(inputs, outputs, processor, id, config_hash, v),
            None => Product::new(inputs, outputs, processor, id, config_hash),
        };

        self.products.push(product);
        self.dependents.push(Vec::new());
        self.dependencies.push(Vec::new());

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
            self.dependents[producer_id].push(consumer_id);
            self.dependencies[consumer_id].push(producer_id);
        }
    }

    /// Topological sort - returns product ids in execution order
    /// Returns error if there's a cycle
    pub fn topological_sort(&self) -> Result<Vec<usize>> {
        let mut in_degree: Vec<usize> = self.dependencies.iter()
            .map(|deps| deps.len())
            .collect();

        // Start with products that have no dependencies
        let mut queue: Vec<usize> = in_degree.iter()
            .enumerate()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(id, _)| id)
            .collect();
        queue.sort_by(|a, b| b.cmp(a));

        let mut result = Vec::new();
        let mut visited = HashSet::new();

        while let Some(id) = queue.pop() {
            if visited.contains(&id) {
                continue;
            }
            visited.insert(id);
            result.push(id);

            // Reduce in-degree of dependents
            let mut newly_ready = Vec::new();
            for &dep_id in &self.dependents[id] {
                in_degree[dep_id] = in_degree[dep_id].saturating_sub(1);
                if in_degree[dep_id] == 0 && !visited.contains(&dep_id) {
                    newly_ready.push(dep_id);
                }
            }
            if !newly_ready.is_empty() {
                queue.extend(newly_ready);
                queue.sort_by(|a, b| b.cmp(a));
            }
        }

        if result.len() != self.products.len() {
            return Err(crate::exit_code::RsbError::new(
                crate::exit_code::RsbExitCode::GraphError,
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

    /// Get dependencies of a product (products that must be built before this one)
    pub fn get_dependencies(&self, id: usize) -> &[usize] {
        &self.dependencies[id]
    }

    /// Get mutable access to a product by id
    pub fn get_product_mut(&mut self, id: usize) -> Option<&mut Product> {
        self.products.get_mut(id)
    }

}

impl Default for BuildGraph {
    fn default() -> Self {
        Self::new()
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
        let mut lines = Vec::new();
        lines.push("digraph build_graph {".to_string());
        lines.push("    rankdir=LR;".to_string());
        lines.push("".to_string());

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
        lines.push("    // Source files".to_string());
        for file in &input_files {
            if !output_files.contains(file) {
                let node_id = Self::path_node_id(file);
                let label = Self::file_label(file);
                lines.push(format!("    {} [label=\"{}\" shape=note style=filled fillcolor=white];", node_id, label));
            }
        }

        lines.push("".to_string());
        lines.push("    // Generated files".to_string());
        for file in &output_files {
            let node_id = Self::path_node_id(file);
            let label = Self::file_label(file);
            let color = if input_files.contains(file) { "lightgreen" } else { "lightyellow" };
            lines.push(format!("    {} [label=\"{}\" shape=note style=filled fillcolor={}];", node_id, label, color));
        }

        lines.push("".to_string());
        lines.push("    // Processors".to_string());
        for product in &self.products {
            let node_id = Self::processor_node_id(product);
            let color = match product.processor.as_str() {
                "tera" => "lightblue",
                "cc_single_file" => "lightsalmon",
                _ => "lightgray",
            };
            lines.push(format!("    {} [label=\"{}\" shape=box style=filled fillcolor={}];",
                node_id, product.processor, color));
        }

        lines.push("".to_string());
        lines.push("    // Edges".to_string());
        for product in &self.products {
            let proc_id = Self::processor_node_id(product);

            // Input files -> processor
            for input in &product.inputs {
                let input_id = Self::path_node_id(input);
                lines.push(format!("    {} -> {};", input_id, proc_id));
            }

            // Processor -> output files
            for output in &product.outputs {
                let output_id = Self::path_node_id(output);
                lines.push(format!("    {} -> {};", proc_id, output_id));
            }
        }

        lines.push("}".to_string());
        lines.join("\n")
    }

    /// Format graph as Mermaid
    /// Only shows primary source files (first input per product), not headers,
    /// to keep the diagram manageable for large projects.
    pub fn to_mermaid(&self) -> String {
        let mut lines = Vec::new();
        lines.push("graph LR".to_string());

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

        lines.push("".to_string());
        lines.push("    %% Source files".to_string());
        for file in &source_files {
            if !output_files.contains(file) {
                let node_id = Self::path_node_id(file);
                let label = Self::file_label(file);
                lines.push(format!("    {}[/\"{}\"/]", node_id, label));
            }
        }

        lines.push("".to_string());
        lines.push("    %% Generated files".to_string());
        for file in &output_files {
            let node_id = Self::path_node_id(file);
            let label = Self::file_label(file);
            lines.push(format!("    {}[/\"{}\"/]", node_id, label));
        }

        lines.push("".to_string());
        lines.push("    %% Processors".to_string());
        for product in &self.products {
            let node_id = Self::processor_node_id(product);
            lines.push(format!("    {}[\"{}\" ]", node_id, product.processor));
        }

        lines.push("".to_string());
        lines.push("    %% Edges".to_string());
        for product in &self.products {
            let proc_id = Self::processor_node_id(product);

            // Only connect primary source file (first input), skip headers
            if let Some(first_input) = product.inputs.first() {
                let input_id = Self::path_node_id(first_input);
                lines.push(format!("    {} --> {}", input_id, proc_id));
            }

            for output in &product.outputs {
                let output_id = Self::path_node_id(output);
                lines.push(format!("    {} --> {}", proc_id, output_id));
            }
        }

        // Add styling
        lines.push("".to_string());
        let tera_procs: Vec<_> = self.products.iter()
            .filter(|p| p.processor == "tera")
            .map(Self::processor_node_id)
            .collect();
        let cc_procs: Vec<_> = self.products.iter()
            .filter(|p| p.processor == "cc_single_file")
            .map(Self::processor_node_id)
            .collect();

        if !tera_procs.is_empty() {
            lines.push(format!("    style {} fill:#add8e6", tera_procs.join(",")));
        }
        if !cc_procs.is_empty() {
            lines.push(format!("    style {} fill:#ffa07a", cc_procs.join(",")));
        }

        lines.join("\n")
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
                    "depends_on": &self.dependencies[product.id],
                })
            })
            .collect();

        let root = serde_json::json!({ "products": nodes });
        serde_json::to_string_pretty(&root).expect("internal error: failed to serialize graph JSON")
    }

    /// Format graph as plain text
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Build Dependency Graph".to_string());
        lines.push("======================".to_string());
        lines.push("".to_string());

        // Get topological order
        let order = match self.topological_sort() {
            Ok(o) => o,
            Err(_) => {
                lines.push("Error: Cycle detected in graph".to_string());
                return lines.join("\n");
            }
        };

        for id in order {
            let product = &self.products[id];
            let inputs: Vec<_> = product.inputs.iter()
                .filter_map(|p| p.file_name())
                .filter_map(|n| n.to_str())
                .collect();
            let outputs: Vec<_> = product.outputs.iter()
                .filter_map(|p| p.file_name())
                .filter_map(|n| n.to_str())
                .collect();

            lines.push(format!("[{}] {} -> {}",
                product.processor,
                inputs.join(", "),
                outputs.join(", ")));

            // Show dependencies
            {
                let deps = &self.dependencies[product.id];
                if !deps.is_empty() {
                    let dep_names: Vec<_> = deps.iter()
                        .map(|&d| {
                            let dep = &self.products[d];
                            let out: Vec<_> = dep.outputs.iter()
                                .filter_map(|p| p.file_name())
                                .filter_map(|n| n.to_str())
                                .collect();
                            out.join(", ")
                        })
                        .collect();
                    lines.push(format!("    depends on: {}", dep_names.join(", ")));
                }
            }
        }

        if self.products.is_empty() {
            lines.push("(empty graph)".to_string());
        }

        lines.join("\n")
    }

    /// Generate SVG by piping DOT through the `dot` command
    pub fn to_svg(&self) -> Result<String> {
        use std::process::{Command, Stdio};
        use std::io::Write;
        use crate::processors::log_command;

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

        child.stdin.take().expect("stdin was piped").write_all(dot_content.as_bytes())?;

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("dot command failed: {}", stderr);
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Generate a self-contained HTML file with Mermaid diagram
    pub fn to_html(&self) -> String {
        let mermaid_content = self.to_mermaid();
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>RSB Build Graph</title>
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
    <h1>RSB Build Graph</h1>
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
