mod a2x;
mod cc_single_file;
mod chromium;
mod drawio;
mod generic;
mod ijinja2;
mod imarkdown2html;
mod ipdfunite;
mod iprotobuf;
mod isass;
mod jinja2;
mod libreoffice;
mod mako;
mod markdown2html;
mod marp;
mod mermaid;
mod objdump;
mod pandoc;
mod pdflatex;
mod pdfunite;
mod protobuf;
mod rust_single_file;
mod sass;
mod yaml2json;

mod requirements;
pub mod tags;
pub mod tera;

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::config::{StandardConfig, output_config_hash};
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::processor::resolve_dep_inputs;

/// Represents a single template file to be processed (shared by tera and mako).
pub(super) struct TemplateItem {
    /// Path to the template file
    pub source_path: PathBuf,
    /// Path where the rendered output will be written
    pub output_path: PathBuf,
}

impl TemplateItem {
    pub const fn new(source_path: PathBuf, output_path: PathBuf) -> Self {
        Self {
            source_path,
            output_path,
        }
    }
}

/// Find all template files matching configured extensions, stripping the
/// extension to produce the output path. Shared by tera and mako processors.
pub(super) fn find_templates(scan: &StandardConfig, file_index: &FileIndex) -> Vec<TemplateItem> {
    let paths = file_index.scan(scan, true);
    let extensions = scan.src_extensions();
    let src_dirs = scan.src_dirs();

    let mut items = Vec::new();
    for path in paths {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for ext in extensions {
            if filename.ends_with(ext.as_str()) {
                let output_name = &filename[..filename.len() - ext.len()];
                if !output_name.is_empty() {
                    // Strip the matching scan_dir prefix to get the output path
                    let output_path = src_dirs
                        .iter()
                        .filter(|d| !d.is_empty())
                        .find_map(|d| {
                            path.strip_prefix(d)
                                .ok()
                                .map(|r| r.with_file_name(output_name))
                        })
                        .unwrap_or_else(|| PathBuf::from(output_name));
                    items.push(TemplateItem::new(path.clone(), output_path));
                    break;
                }
            }
        }
    }

    items
}

/// The files under `root` with extension `ext`, honouring the standard
/// `src_exclude_dirs`/`src_exclude_files`/`src_exclude_paths`/`src_files`
/// fields of `scan` exactly as `FileIndex::scan` does. For a processor
/// that locates its sources by its own directory field (pdfunite's
/// `source_dir`) rather than by `src_dirs`; passing empty lists to
/// `FileIndex::query` instead silently ignored every exclude the user set.
pub(super) fn query_under(
    file_index: &FileIndex,
    root: &Path,
    ext: &str,
    scan: &StandardConfig,
) -> Vec<PathBuf> {
    fn as_strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }
    let exclude_dirs = as_strs(scan.src_exclude_dirs());
    let exclude_files = as_strs(scan.src_exclude_files());
    let exclude_paths = as_strs(scan.src_exclude_paths());
    let src_files = as_strs(scan.src_files());
    file_index.query(
        root,
        &[ext],
        &exclude_dirs,
        &exclude_files,
        &exclude_paths,
        &src_files,
    )
}

/// Parameters shared by multi-format and single-format discover helpers.
pub struct DiscoverParams<'a, C: Serialize> {
    pub scan: &'a StandardConfig,
    /// Extra inputs of every product. `scan.dep_auto` is merged in by the
    /// helpers ([`resolve_dep_inputs`]), so a caller passes its `dep_inputs`
    /// (or a computed superset of them), never a list that already carries
    /// the `dep_auto` files.
    pub dep_inputs: &'a [String],
    pub config: &'a C,
    pub output_dir: &'a str,
    pub processor_name: &'a str,
    /// Allowlist of field names to include in the config-change checksum,
    /// derived from the plugin's `FieldSpec` list (`checksum_fields_of`).
    pub checksum_fields: Vec<&'static str>,
    /// Keep only the scanned sources this returns `true` for; `None` keeps
    /// every scanned source. See `SimpleGeneratorParams::source_filter`.
    pub source_filter: Option<fn(&C, &Path) -> bool>,
}

/// Compute the output path for a source file.
///
/// Strips the matching `src_dirs` prefix from the source path, replaces the extension,
/// and joins the result under `output_dir`. This is the single place where
/// source-to-output path mapping is defined.
pub(super) fn output_path(
    source: &Path,
    src_dirs: &[String],
    output_dir: &str,
    extension: &str,
) -> PathBuf {
    let full_parent = super::parent_dir_or_empty(source);
    let parent = src_dirs
        .iter()
        .filter(|d| !d.is_empty())
        .find_map(|d| full_parent.strip_prefix(d).ok())
        .unwrap_or(full_parent);
    let stem = source.file_stem().unwrap_or_default();
    let output_name = format!("{}.{}", stem.to_string_lossy(), extension);
    Path::new(output_dir).join(parent).join(output_name)
}

/// Discover one product per source x format pair. Returns Ok(()) immediately
/// if the scan root is invalid (directory doesn't exist).
pub fn discover_multi_format(
    graph: &mut BuildGraph,
    file_index: &FileIndex,
    params: &DiscoverParams<'_, impl Serialize>,
    formats: &[String],
) -> Result<()> {
    let Some(files) = crate::processor::scan_or_skip(params.scan, file_index) else {
        return Ok(());
    };

    let hash = Some(output_config_hash(params.config, &params.checksum_fields));
    let extra = resolve_dep_inputs(params.dep_inputs, &params.scan.dep_auto)?;
    let src_dirs = params.scan.src_dirs();

    for source in &files {
        if let Some(keep) = params.source_filter
            && !keep(params.config, source)
        {
            continue;
        }
        for format in formats {
            let output = output_path(source, src_dirs, params.output_dir, format);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source.clone());
            inputs.extend_from_slice(&extra);

            graph.add_product_with(
                inputs,
                vec![output],
                params.processor_name,
                hash.clone(),
                Some(format),
                Vec::new(),
            )?;
        }
    }

    Ok(())
}

/// Discover one product per source file. Returns Ok(()) immediately
/// if the scan root is invalid (directory doesn't exist).
pub fn discover_single_format(
    graph: &mut BuildGraph,
    file_index: &FileIndex,
    params: &DiscoverParams<'_, impl Serialize>,
    extension: &str,
) -> Result<()> {
    let format = extension.to_owned();
    discover_multi_format(graph, file_index, params, &[format])
}
