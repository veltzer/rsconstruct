mod a2x;
mod cc_single_file;
mod drawio;
mod libreoffice;
mod linux_module;
mod mako;
mod marp;
mod markdown;
mod mermaid;
mod pandoc;
mod pdflatex;
mod pdfunite;
pub mod tags;
mod tera;

pub use a2x::A2xProcessor;
pub use cc_single_file::CcSingleFileProcessor;
pub use drawio::DrawioProcessor;
pub use libreoffice::LibreofficeProcessor;
pub use linux_module::LinuxModuleProcessor;
pub use mako::MakoProcessor;
pub use marp::MarpProcessor;
pub use markdown::MarkdownProcessor;
pub use mermaid::MermaidProcessor;
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
use crate::processors::scan_root_valid;

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

/// Discover one product per source x format pair. Returns Ok(()) immediately
/// if the scan root is invalid (directory doesn't exist).
pub(super) fn discover_multi_format(
    graph: &mut BuildGraph,
    file_index: &FileIndex,
    params: &DiscoverParams<'_, impl Serialize>,
    formats: &[String],
) -> Result<()> {
    if !scan_root_valid(params.scan) {
        return Ok(());
    }

    let files = file_index.scan(params.scan, true);
    if files.is_empty() {
        return Ok(());
    }

    let hash = Some(config_hash(params.config));
    let extra = resolve_extra_inputs(params.extra_inputs)?;

    for source in &files {
        for format in formats {
            let stem = source.file_stem().unwrap_or_default();
            let parent = source.parent().unwrap_or(Path::new(""));
            let output_name = format!("{}.{}", stem.to_string_lossy(), format);
            let output = Path::new(params.output_dir).join(format.as_str()).join(parent).join(output_name);

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
    if !scan_root_valid(params.scan) {
        return Ok(());
    }

    let files = file_index.scan(params.scan, true);
    if files.is_empty() {
        return Ok(());
    }

    let hash = Some(config_hash(params.config));
    let extra = resolve_extra_inputs(params.extra_inputs)?;

    for source in files {
        let stem = source.file_stem().unwrap_or_default();
        let parent = source.parent().unwrap_or(Path::new(""));
        let output_name = format!("{}.{}", stem.to_string_lossy(), extension);
        let output = Path::new(params.output_dir).join(parent).join(output_name);

        let mut inputs = Vec::with_capacity(1 + extra.len());
        inputs.push(source);
        inputs.extend_from_slice(&extra);

        graph.add_product(inputs, vec![output], params.processor_name, hash.clone())?;
    }

    Ok(())
}
