use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::checksum::ChecksumCache;

/// A single build product with concrete inputs and outputs
#[derive(Debug, Clone)]
pub struct Product {
    /// Input files (real paths)
    pub inputs: Vec<PathBuf>,
    /// Output files (real paths)
    pub outputs: Vec<PathBuf>,
    /// Which processor handles this product
    pub processor: String,
    /// Unique identifier for this product
    pub id: usize,
}

impl Product {
    pub fn new(inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str, id: usize) -> Self {
        Self {
            inputs,
            outputs,
            processor: processor.to_string(),
            id,
        }
    }

    /// Display name for logging
    pub fn display(&self) -> String {
        let inputs: Vec<_> = self.inputs.iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();
        let outputs: Vec<_> = self.outputs.iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();
        format!("input: {}, output: {}", inputs.join(", "), outputs.join(", "))
    }

    /// Cache key for checksum tracking
    pub fn cache_key(&self) -> String {
        let inputs: Vec<_> = self.inputs.iter()
            .map(|p| p.display().to_string())
            .collect();
        format!("{}:{}", self.processor, inputs.join(":"))
    }
}

/// Build graph with dependency resolution
pub struct BuildGraph {
    products: Vec<Product>,
    /// Map from output path to product id
    output_to_product: HashMap<PathBuf, usize>,
    /// Adjacency list: product id -> list of product ids that depend on it
    dependents: HashMap<usize, Vec<usize>>,
    /// Reverse: product id -> list of product ids it depends on
    dependencies: HashMap<usize, Vec<usize>>,
}

impl BuildGraph {
    pub fn new() -> Self {
        Self {
            products: Vec::new(),
            output_to_product: HashMap::new(),
            dependents: HashMap::new(),
            dependencies: HashMap::new(),
        }
    }

    /// Add a product to the graph
    pub fn add_product(&mut self, inputs: Vec<PathBuf>, outputs: Vec<PathBuf>, processor: &str) -> usize {
        let id = self.products.len();
        let product = Product::new(inputs, outputs.clone(), processor, id);

        // Register outputs
        for output in &outputs {
            self.output_to_product.insert(output.clone(), id);
        }

        self.products.push(product);
        self.dependents.insert(id, Vec::new());
        self.dependencies.insert(id, Vec::new());

        id
    }

    /// Resolve dependencies between products
    pub fn resolve_dependencies(&mut self) {
        // For each product, check if any of its inputs are outputs of other products
        for product in &self.products {
            for input in &product.inputs {
                if let Some(&producer_id) = self.output_to_product.get(input) {
                    if producer_id != product.id {
                        // producer_id produces something that product.id needs
                        self.dependents.get_mut(&producer_id).unwrap().push(product.id);
                        self.dependencies.get_mut(&product.id).unwrap().push(producer_id);
                    }
                }
            }
        }
    }

    /// Topological sort - returns product ids in execution order
    /// Returns error if there's a cycle
    pub fn topological_sort(&self) -> Result<Vec<usize>> {
        let mut in_degree: HashMap<usize, usize> = HashMap::new();

        // Initialize in-degrees
        for product in &self.products {
            in_degree.insert(product.id, self.dependencies.get(&product.id).map_or(0, |d| d.len()));
        }

        // Start with products that have no dependencies
        let mut queue: Vec<usize> = in_degree.iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();
        let mut visited = HashSet::new();

        while let Some(id) = queue.pop() {
            if visited.contains(&id) {
                continue;
            }
            visited.insert(id);
            result.push(id);

            // Reduce in-degree of dependents
            if let Some(deps) = self.dependents.get(&id) {
                for &dep_id in deps {
                    if let Some(deg) = in_degree.get_mut(&dep_id) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 && !visited.contains(&dep_id) {
                            queue.push(dep_id);
                        }
                    }
                }
            }
        }

        if result.len() != self.products.len() {
            bail!("Cycle detected in build graph");
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

    /// Check if a product needs rebuilding based on checksums
    pub fn needs_rebuild(&self, product: &Product, cache: &ChecksumCache, force: bool) -> Result<bool> {
        if force {
            return Ok(true);
        }

        let cache_key = product.cache_key();

        // Calculate combined checksum of all inputs
        let mut checksums = Vec::new();
        for input in &product.inputs {
            if input.exists() {
                checksums.push(ChecksumCache::calculate_checksum(input)?);
            } else {
                // Input doesn't exist yet, needs rebuild
                return Ok(true);
            }
        }
        let combined = checksums.join(":");

        // Check if outputs exist
        for output in &product.outputs {
            if !output.exists() {
                return Ok(true);
            }
        }

        // Check cache
        if let Some(cached) = cache.get_by_key(&cache_key) {
            if cached == &combined {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Update cache after successful build
    pub fn update_cache(&self, product: &Product, cache: &mut ChecksumCache) -> Result<()> {
        let cache_key = product.cache_key();

        let mut checksums = Vec::new();
        for input in &product.inputs {
            checksums.push(ChecksumCache::calculate_checksum(input)?);
        }
        let combined = checksums.join(":");

        cache.set_by_key(cache_key, combined);
        Ok(())
    }
}

impl Default for BuildGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildGraph {
    /// Generate a short label for a product (used in graph output)
    fn product_label(&self, product: &Product) -> String {
        let outputs: Vec<_> = product.outputs.iter()
            .filter_map(|p| p.file_name())
            .filter_map(|n| n.to_str())
            .collect();
        format!("[{}] {}", product.processor, outputs.join(", "))
    }

    /// Generate a node ID for a product (safe for DOT/Mermaid)
    fn product_node_id(&self, product: &Product) -> String {
        format!("p{}", product.id)
    }

    /// Format graph as DOT (Graphviz)
    pub fn to_dot(&self) -> String {
        let mut lines = Vec::new();
        lines.push("digraph build_graph {".to_string());
        lines.push("    rankdir=LR;".to_string());
        lines.push("    node [shape=box];".to_string());
        lines.push("".to_string());

        // Add nodes
        for product in &self.products {
            let node_id = self.product_node_id(product);
            let label = self.product_label(product);
            let color = match product.processor.as_str() {
                "template" => "lightblue",
                "lint" => "lightyellow",
                _ => "lightgray",
            };
            lines.push(format!("    {} [label=\"{}\" style=filled fillcolor={}];",
                node_id, label, color));
        }

        lines.push("".to_string());

        // Add edges
        for product in &self.products {
            if let Some(deps) = self.dependents.get(&product.id) {
                for &dep_id in deps {
                    let from = self.product_node_id(product);
                    let to = self.product_node_id(&self.products[dep_id]);
                    lines.push(format!("    {} -> {};", from, to));
                }
            }
        }

        lines.push("}".to_string());
        lines.join("\n")
    }

    /// Format graph as Mermaid
    pub fn to_mermaid(&self) -> String {
        let mut lines = Vec::new();
        lines.push("graph LR".to_string());

        // Add nodes with styling
        for product in &self.products {
            let node_id = self.product_node_id(product);
            let label = self.product_label(product);
            lines.push(format!("    {}[\"{}\" ]", node_id, label));
        }

        lines.push("".to_string());

        // Add edges
        for product in &self.products {
            if let Some(deps) = self.dependents.get(&product.id) {
                for &dep_id in deps {
                    let from = self.product_node_id(product);
                    let to = self.product_node_id(&self.products[dep_id]);
                    lines.push(format!("    {} --> {}", from, to));
                }
            }
        }

        // Add styling
        lines.push("".to_string());
        let template_nodes: Vec<_> = self.products.iter()
            .filter(|p| p.processor == "template")
            .map(|p| self.product_node_id(p))
            .collect();
        let lint_nodes: Vec<_> = self.products.iter()
            .filter(|p| p.processor == "lint")
            .map(|p| self.product_node_id(p))
            .collect();

        if !template_nodes.is_empty() {
            lines.push(format!("    style {} fill:#add8e6", template_nodes.join(",")));
        }
        if !lint_nodes.is_empty() {
            lines.push(format!("    style {} fill:#ffffe0", lint_nodes.join(",")));
        }

        lines.join("\n")
    }

    /// Format graph as JSON
    pub fn to_json(&self) -> String {
        let mut nodes = Vec::new();
        for product in &self.products {
            let inputs: Vec<_> = product.inputs.iter()
                .map(|p| p.display().to_string())
                .collect();
            let outputs: Vec<_> = product.outputs.iter()
                .map(|p| p.display().to_string())
                .collect();
            nodes.push(format!(
                r#"    {{
      "id": {},
      "processor": "{}",
      "inputs": {:?},
      "outputs": {:?},
      "depends_on": {:?}
    }}"#,
                product.id,
                product.processor,
                inputs,
                outputs,
                self.dependencies.get(&product.id).unwrap_or(&Vec::new())
            ));
        }

        format!("{{\n  \"products\": [\n{}\n  ]\n}}", nodes.join(",\n"))
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
            if let Some(deps) = self.dependencies.get(&product.id) {
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
}
