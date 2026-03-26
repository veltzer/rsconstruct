/// Generate boilerplate for a simple generator processor.
///
/// Generates the struct, `new()` constructor, and the boilerplate `ProductDiscovery` methods
/// (`description`, `processor_type`, `auto_detect`, `required_tools`, `discover`, `clean`,
/// `config_json`). The `execute()` method must be provided via `execute_product()` in a
/// separate `impl` block.
///
/// # Variants
/// - Multi-format: `discover: multi_format, formats_field: formats, ...`
/// - Single-format: `discover: single_format, extension: "pdf", ...`
///
/// # Tool specification
/// - `tool_field: field_name` — tool name from config field
/// - `tool_field_extra: field_name ["extra".to_string()]` — config field + extra tools
/// - `tools: ["tool".to_string()]` — static tool list
macro_rules! impl_generator {
    // Multi-format variant
    ($processor:ident, $config:ty,
     description: $desc:expr,
     name: $name:expr,
     discover: multi_format, formats_field: $formats:ident,
     $($tool_spec:tt)*
    ) => {
        pub struct $processor {
            config: $config,
        }

        impl $processor {
            pub fn new(config: $config) -> Self {
                Self { config }
            }
        }

        impl $crate::processors::ProductDiscovery for $processor {
            fn description(&self) -> &str { $desc }

            fn processor_type(&self) -> $crate::processors::ProcessorType {
                $crate::processors::ProcessorType::Generator
            }

            fn auto_detect(&self, file_index: &$crate::file_index::FileIndex) -> bool {
                $crate::processors::scan_root_valid(&self.config.scan)
                    && !file_index.scan(&self.config.scan, true).is_empty()
            }

            fn required_tools(&self) -> Vec<String> {
                impl_generator!(@tools self, $($tool_spec)*)
            }

            fn discover(
                &self,
                graph: &mut $crate::graph::BuildGraph,
                file_index: &$crate::file_index::FileIndex,
            ) -> anyhow::Result<()> {
                let params = super::DiscoverParams {
                    scan: &self.config.scan,
                    extra_inputs: &self.config.extra_inputs,
                    config: &self.config,
                    output_dir: &self.config.output_dir,
                    processor_name: $name,
                };
                super::discover_multi_format(graph, file_index, &params, &self.config.$formats)
            }

            fn execute(&self, product: &$crate::graph::Product) -> anyhow::Result<()> {
                self.execute_product(product)
            }

            fn clean(&self, product: &$crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
                $crate::processors::clean_outputs(product, $name, verbose)
            }

            fn config_json(&self) -> Option<String> {
                serde_json::to_string(&self.config).ok()
            }
        }
    };

    // Single-format variant
    ($processor:ident, $config:ty,
     description: $desc:expr,
     name: $name:expr,
     discover: single_format, extension: $ext:expr,
     $($tool_spec:tt)*
    ) => {
        pub struct $processor {
            config: $config,
        }

        impl $processor {
            pub fn new(config: $config) -> Self {
                Self { config }
            }
        }

        impl $crate::processors::ProductDiscovery for $processor {
            fn description(&self) -> &str { $desc }

            fn processor_type(&self) -> $crate::processors::ProcessorType {
                $crate::processors::ProcessorType::Generator
            }

            fn auto_detect(&self, file_index: &$crate::file_index::FileIndex) -> bool {
                $crate::processors::scan_root_valid(&self.config.scan)
                    && !file_index.scan(&self.config.scan, true).is_empty()
            }

            fn required_tools(&self) -> Vec<String> {
                impl_generator!(@tools self, $($tool_spec)*)
            }

            fn discover(
                &self,
                graph: &mut $crate::graph::BuildGraph,
                file_index: &$crate::file_index::FileIndex,
            ) -> anyhow::Result<()> {
                let params = super::DiscoverParams {
                    scan: &self.config.scan,
                    extra_inputs: &self.config.extra_inputs,
                    config: &self.config,
                    output_dir: &self.config.output_dir,
                    processor_name: $name,
                };
                super::discover_single_format(graph, file_index, &params, $ext)
            }

            fn execute(&self, product: &$crate::graph::Product) -> anyhow::Result<()> {
                self.execute_product(product)
            }

            fn clean(&self, product: &$crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
                $crate::processors::clean_outputs(product, $name, verbose)
            }

            fn config_json(&self) -> Option<String> {
                serde_json::to_string(&self.config).ok()
            }
        }
    };

    // --- Tool specification dispatch ---
    (@tools $self:ident, tool_field: $field:ident) => {
        vec![$self.config.$field.clone()]
    };
    (@tools $self:ident, tool_field_extra: $field:ident [$($extra:expr),+]) => {
        vec![$self.config.$field.clone(), $($extra),+]
    };
    (@tools $self:ident, tools: [$($tool:expr),+]) => {
        vec![$($tool),+]
    };
}

mod a2x;
mod cc_single_file;
mod chromium;
mod drawio;
mod libreoffice;
mod linux_module;
mod mako;
mod marp;
mod markdown;
mod mermaid;
mod objdump;
mod pandoc;
mod pdflatex;
mod pdfunite;
pub mod tags;
mod tera;

pub use a2x::A2xProcessor;
pub use cc_single_file::CcSingleFileProcessor;
pub use chromium::ChromiumProcessor;
pub use drawio::DrawioProcessor;
pub use libreoffice::LibreofficeProcessor;
pub use linux_module::LinuxModuleProcessor;
pub use mako::MakoProcessor;
pub use marp::MarpProcessor;
pub use markdown::MarkdownProcessor;
pub use mermaid::MermaidProcessor;
pub use objdump::ObjdumpProcessor;
pub use pandoc::PandocProcessor;
pub use pdflatex::PdflatexProcessor;
pub use pdfunite::PdfuniteProcessor;
pub use tags::TagsProcessor;
pub use tera::TeraProcessor;

use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::Serialize;

use crate::config::{ScanConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;

/// Represents a single template file to be processed (shared by tera and mako).
pub(super) struct TemplateItem {
    /// Path to the template file
    pub source_path: PathBuf,
    /// Path where the rendered output will be written
    pub output_path: PathBuf,
}

impl TemplateItem {
    pub fn new(source_path: PathBuf, output_path: PathBuf) -> Self {
        Self {
            source_path,
            output_path,
        }
    }
}

/// Find all template files matching configured extensions, stripping the
/// extension to produce the output path. Shared by tera and mako processors.
pub(super) fn find_templates(scan: &ScanConfig, file_index: &FileIndex) -> Result<Vec<TemplateItem>> {
    let paths = file_index.scan(scan, true);
    let extensions = scan.extensions();
    let scan_dir = scan.scan_dir();

    let mut items = Vec::new();
    for path in paths {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for ext in extensions {
            if filename.ends_with(ext.as_str()) {
                let output_name = &filename[..filename.len() - ext.len()];
                if !output_name.is_empty() {
                    // Strip the scan_dir prefix to get the output path
                    let output_path = if !scan_dir.is_empty() {
                        if let Ok(relative) = path.strip_prefix(scan_dir) {
                            relative.with_file_name(output_name)
                        } else {
                            PathBuf::from(output_name)
                        }
                    } else {
                        PathBuf::from(output_name)
                    };
                    items.push(TemplateItem::new(path.clone(), output_path));
                    break;
                }
            }
        }
    }

    Ok(items)
}

/// Parameters shared by multi-format and single-format discover helpers.
pub(super) struct DiscoverParams<'a, C: Serialize> {
    pub scan: &'a ScanConfig,
    pub extra_inputs: &'a [String],
    pub config: &'a C,
    pub output_dir: &'a str,
    pub processor_name: &'a str,
}

/// Compute the output path for a source file.
///
/// Strips the `scan_dir` prefix from the source path, replaces the extension,
/// and joins the result under `output_dir`. This is the single place where
/// source-to-output path mapping is defined.
pub(super) fn output_path(source: &Path, scan_dir: &str, output_dir: &str, extension: &str) -> PathBuf {
    let scan_root = Path::new(scan_dir);
    let full_parent = source.parent().unwrap_or(Path::new(""));
    let parent = full_parent.strip_prefix(scan_root).unwrap_or(full_parent);
    let stem = source.file_stem().unwrap_or_default();
    let output_name = format!("{}.{}", stem.to_string_lossy(), extension);
    Path::new(output_dir).join(parent).join(output_name)
}

/// Discover one product per source x format pair. Returns Ok(()) immediately
/// if the scan root is invalid (directory doesn't exist).
pub(super) fn discover_multi_format(
    graph: &mut BuildGraph,
    file_index: &FileIndex,
    params: &DiscoverParams<'_, impl Serialize>,
    formats: &[String],
) -> Result<()> {
    let Some(files) = crate::processors::scan_or_skip(params.scan, file_index) else {
        return Ok(());
    };

    let hash = Some(config_hash(params.config));
    let extra = resolve_extra_inputs(params.extra_inputs)?;
    let scan_dir = params.scan.scan_dir();

    for source in &files {
        for format in formats {
            let output = output_path(source, scan_dir, params.output_dir, format);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, vec![output], params.processor_name, hash.clone())?;
        }
    }

    Ok(())
}

/// Discover one product per source file. Returns Ok(()) immediately
/// if the scan root is invalid (directory doesn't exist).
pub(super) fn discover_single_format(
    graph: &mut BuildGraph,
    file_index: &FileIndex,
    params: &DiscoverParams<'_, impl Serialize>,
    extension: &str,
) -> Result<()> {
    let format = extension.to_owned();
    discover_multi_format(graph, file_index, params, &[format])
}
