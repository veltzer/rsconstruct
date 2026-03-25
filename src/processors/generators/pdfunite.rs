use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PdfuniteConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, run_command, check_command_output};

use super::marp::cleanup_marp_tmp_dirs;

pub struct PdfuniteProcessor {
    config: PdfuniteConfig,
}

impl PdfuniteProcessor {
    pub fn new(config: PdfuniteConfig) -> Self {
        Self { config }
    }
}

/// Convert a directory name like "advanced-python" to a human-readable title like "Advanced Python".
fn course_title(dir_name: &str) -> String {
    dir_name
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Generate a single-page title PDF using marp.
///
/// Creates a temporary marp markdown file with the course title, renders it to PDF,
/// and returns the path to the generated title page PDF.
fn generate_title_page(marp_bin: &str, title: &str, output_dir: &Path, course_name: &str) -> Result<PathBuf> {
    let title_pdf = output_dir.join(format!("{}-title.pdf", course_name));
    let title_md = output_dir.join(format!("{}-title.md", course_name));

    let md_content = format!(
        "---\nmarp: true\npaginate: false\n---\n\n<!-- _class: lead -->\n\n# {}\n",
        title
    );

    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create title page output directory: {}", output_dir.display()))?;

    fs::write(&title_md, &md_content)
        .with_context(|| format!("Failed to write title page markdown: {}", title_md.display()))?;

    let mut cmd = Command::new(marp_bin);
    cmd.arg("--pdf")
        .arg("--output").arg(&title_pdf)
        .arg("--allow-local-files")
        .arg(&title_md);

    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("marp title page for {}", course_name))?;

    cleanup_marp_tmp_dirs();

    // Clean up the temporary markdown file
    let _ = fs::remove_file(&title_md);

    Ok(title_pdf)
}

impl ProductDiscovery for PdfuniteProcessor {
    fn description(&self) -> &str {
        "Merge PDFs from subdirectories into course bundles"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        let base = Path::new(&self.config.source_dir);
        if !base.exists() {
            return false;
        }
        // Check if any subdirectory contains source files
        let ext = &self.config.source_ext;
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|ft| ft.is_dir())
                    && let Ok(files) = fs::read_dir(entry.path())
                {
                    for file in files.flatten() {
                        if file.path().extension().is_some_and(|e| {
                            let dot_ext = format!(".{}", e.to_string_lossy());
                            dot_ext == *ext
                        }) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.pdfunite_bin.clone()];
        if self.config.title_page {
            tools.push(self.config.marp_bin.clone());
        }
        tools
    }

    fn discover(&self, graph: &mut BuildGraph, _file_index: &FileIndex) -> Result<()> {
        let base = Path::new(&self.config.source_dir);
        if !base.exists() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;
        let ext = self.config.source_ext.strip_prefix('.').unwrap_or(&self.config.source_ext);

        let mut subdirs: Vec<_> = fs::read_dir(base)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
            .collect();
        subdirs.sort_by_key(|e| e.file_name());

        for entry in subdirs {
            let subdir_name = entry.file_name();

            // Find all source files in this subdirectory
            let mut source_files: Vec<PathBuf> = Vec::new();
            for file_entry in fs::read_dir(entry.path())?.filter_map(|e| e.ok()) {
                let path = file_entry.path();
                if path.extension().is_some_and(|e| e == ext) {
                    source_files.push(path);
                }
            }
            if source_files.is_empty() {
                continue;
            }
            source_files.sort();

            // Compute the expected PDF paths from the upstream processor
            let inputs: Vec<PathBuf> = source_files.iter().map(|src| {
                let stem = src.file_stem().expect(crate::errors::PATH_NO_STEM);
                let parent = src.parent().expect(crate::errors::PATH_NO_PARENT);
                Path::new(&self.config.source_output_dir)
                    .join(parent)
                    .join(format!("{}.pdf", stem.to_string_lossy()))
            }).chain(extra.iter().cloned()).collect();

            let course_name = subdir_name.to_string_lossy();
            let mut outputs = vec![
                Path::new(&self.config.output_dir).join(format!("{}.pdf", course_name)),
            ];
            // Register the title page PDF as an additional output so clean removes it
            if self.config.title_page {
                outputs.push(
                    Path::new(&self.config.output_dir).join(format!("{}-title.pdf", course_name)),
                );
            }

            graph.add_product(inputs, outputs, crate::processors::names::PDFUNITE, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let output = product.primary_output();

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create pdfunite output directory: {}", parent.display()))?;
        }

        // Generate title page if enabled
        let title_pdf = if self.config.title_page {
            let course_name = output.file_stem()
                .context("pdfunite output has no file stem")?
                .to_string_lossy();
            let title = course_title(&course_name);
            let output_dir = output.parent().context("pdfunite output has no parent")?;
            Some(generate_title_page(&self.config.marp_bin, &title, output_dir, &course_name)?)
        } else {
            None
        };

        let mut cmd = Command::new(&self.config.pdfunite_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        // Title page goes first
        if let Some(ref title) = title_pdf {
            cmd.arg(title);
        }
        // pdfunite takes input files followed by output file
        for input in &product.inputs {
            cmd.arg(input);
        }
        cmd.arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("pdfunite {}", output.display()))
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::PDFUNITE, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
