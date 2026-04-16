use std::collections::HashSet;
use anyhow::Result;
use crate::color;
use super::Builder;

impl Builder {
    /// Run fix mode on all (or filtered) checker processors that have fix capability.
    pub fn fix(&self, ctx: &crate::build_context::BuildContext, processor_filter: Option<&[String]>) -> Result<()> {
        let processors = self.create_processors()?;
        let mut graph = self.build_graph_with_processors(ctx, &processors)?;

        // Filter to only processors with fix capability
        let filter_set: Option<HashSet<&str>> = processor_filter
            .map(|names| names.iter().map(|s| s.as_str()).collect());

        let fixable: Vec<&str> = processors.keys()
            .filter(|name| {
                let proc = &processors[name.as_str()];
                if !proc.can_fix() {
                    return false;
                }
                if let Some(ref filter) = filter_set {
                    return filter.contains(name.as_str());
                }
                true
            })
            .map(|s| s.as_str())
            .collect();

        if fixable.is_empty() {
            if processor_filter.is_some() {
                println!("{}", color::yellow("No matching processors with fix capability."));
            } else {
                println!("{}", color::yellow("No processors with fix capability found."));
            }
            return Ok(());
        }

        // Filter graph to only fixable products
        let fixable_set: HashSet<&str> = fixable.iter().copied().collect();
        graph.retain_products(|p| fixable_set.contains(p.processor.as_str()));

        let products: Vec<_> = graph.products().to_vec();
        if products.is_empty() {
            println!("{}", color::yellow("No files to fix."));
            return Ok(());
        }

        println!("{}", color::bold(&format!(
            "Fixing {} file(s) using: {}",
            products.len(),
            fixable.join(", "),
        )));

        let mut fixed_count = 0usize;
        let mut error_count = 0usize;

        // Group products by processor for batch execution
        let mut by_processor: std::collections::BTreeMap<&str, Vec<&crate::graph::Product>> = std::collections::BTreeMap::new();
        for product in &products {
            by_processor.entry(&product.processor).or_default().push(product);
        }

        for (proc_name, proc_products) in &by_processor {
            let processor = match processors.get(*proc_name) {
                Some(p) => p,
                None => continue,
            };

            if processor.supports_fix_batch() && proc_products.len() > 1 {
                // Batch fix
                let refs: Vec<&crate::graph::Product> = proc_products.iter().copied().collect();
                let results = processor.fix_batch(ctx, &refs);
                for result in results {
                    match result {
                        Ok(()) => fixed_count += 1,
                        Err(e) => {
                            eprintln!("{}: {}", color::red(&format!("[{}] fix error", proc_name)), e);
                            error_count += 1;
                        }
                    }
                }
            } else {
                // Single-file fix
                for product in proc_products {
                    match processor.fix(ctx, product) {
                        Ok(()) => fixed_count += 1,
                        Err(e) => {
                            eprintln!("{}: {}", color::red(&format!("[{}] fix error", proc_name)), e);
                            error_count += 1;
                        }
                    }
                }
            }
        }

        if error_count > 0 {
            println!("{}", color::red(&format!(
                "Fix completed: {} fixed, {} errors",
                fixed_count, error_count,
            )));
            anyhow::bail!("Fix failed with {} error(s)", error_count);
        } else {
            println!("{}", color::green(&format!(
                "Fix completed: {} file(s) processed",
                fixed_count,
            )));
        }

        Ok(())
    }
}
