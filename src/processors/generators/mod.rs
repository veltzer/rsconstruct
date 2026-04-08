mod a2x;
mod cc_single_file;
mod chromium;
mod drawio;
mod explicit;
mod generator;
mod imarkdown2html;
mod ipdfunite;
mod isass;
mod jinja2;
mod libreoffice;
mod linux_module;
mod mako;
mod marp;
mod markdown2html;
mod mermaid;
mod objdump;
mod pandoc;
mod pdflatex;
mod pdfunite;
mod protobuf;
mod rust_single_file;
mod sass;
pub mod tags;
mod tera;
mod yaml2json;

pub use a2x::A2xProcessor;
pub use cc_single_file::CcSingleFileProcessor;
pub use explicit::ExplicitProcessor;
pub use generator::GeneratorProcessor;
pub use imarkdown2html::Imarkdown2htmlProcessor;
pub use ipdfunite::IpdfuniteProcessor;
pub use isass::IsassProcessor;
pub use jinja2::Jinja2Processor;
pub use chromium::ChromiumProcessor;
pub use drawio::DrawioProcessor;
pub use libreoffice::LibreofficeProcessor;
pub use linux_module::LinuxModuleProcessor;
pub use mako::MakoProcessor;
pub use marp::MarpProcessor;
pub use markdown2html::Markdown2htmlProcessor;
pub use mermaid::MermaidProcessor;
pub use objdump::ObjdumpProcessor;
pub use pandoc::PandocProcessor;
pub use pdflatex::PdflatexProcessor;
pub use pdfunite::PdfuniteProcessor;
pub use protobuf::ProtobufProcessor;
pub use rust_single_file::RustSingleFileProcessor;
pub use sass::SassProcessor;
pub use tags::TagsProcessor;
pub use tera::TeraProcessor;
pub use yaml2json::Yaml2jsonProcessor;

use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::Serialize;

use crate::config::{ScanConfig, output_config_hash, resolve_extra_inputs};
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
    let scan_dirs = scan.scan_dirs();

    let mut items = Vec::new();
    for path in paths {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for ext in extensions {
            if filename.ends_with(ext.as_str()) {
                let output_name = &filename[..filename.len() - ext.len()];
                if !output_name.is_empty() {
                    // Strip the matching scan_dir prefix to get the output path
                    let output_path = scan_dirs.iter()
                        .filter(|d| !d.is_empty())
                        .find_map(|d| path.strip_prefix(d).ok().map(|r| r.with_file_name(output_name)))
                        .unwrap_or_else(|| PathBuf::from(output_name));
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

/// Recursively find directories under `base` that contain files with the given extension.
/// Results are sorted for deterministic output.
/// Shared by pdfunite and ipdfunite processors.
pub(super) fn find_dirs_with_ext(base: &Path, ext: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_dirs_with_ext(base, ext, &mut result);
    result.sort();
    result
}

fn collect_dirs_with_ext(dir: &Path, ext: &str, result: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut has_matching_file = false;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            subdirs.push(entry.path());
        } else if !has_matching_file && ft.is_file() {
            if entry.path().extension().is_some_and(|e| e == ext) {
                has_matching_file = true;
            }
        }
    }
    if has_matching_file {
        result.push(dir.to_path_buf());
    }
    for subdir in subdirs {
        collect_dirs_with_ext(&subdir, ext, result);
    }
}

/// Compute the output path for a source file.
///
/// Strips the matching `scan_dirs` prefix from the source path, replaces the extension,
/// and joins the result under `output_dir`. This is the single place where
/// source-to-output path mapping is defined.
pub(super) fn output_path(source: &Path, scan_dirs: &[String], output_dir: &str, extension: &str) -> PathBuf {
    let full_parent = source.parent().unwrap_or(Path::new(""));
    let parent = scan_dirs.iter()
        .filter(|d| !d.is_empty())
        .find_map(|d| full_parent.strip_prefix(d).ok())
        .unwrap_or(full_parent);
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

    let hash = Some(output_config_hash(params.config, &["formats", "output_dir"]));
    let extra = resolve_extra_inputs(params.extra_inputs)?;
    let scan_dirs = params.scan.scan_dirs();

    for source in &files {
        for format in formats {
            let output = output_path(source, scan_dirs, params.output_dir, format);

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
