use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PdflatexConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_command, check_command_output};

/// Temp file extensions produced by pdflatex that should be cleaned between runs.
const PDFLATEX_TEMP_EXTENSIONS: &[&str] = &[".log", ".out", ".toc", ".aux", ".nav", ".snm", ".vrb"];

pub struct PdflatexProcessor {
    config: PdflatexConfig,
}

impl PdflatexProcessor {
    pub fn new(config: PdflatexConfig) -> Self {
        Self { config }
    }

    fn should_process(&self) -> bool {
        scan_root_valid(&self.config.scan)
    }

    fn output_path(&self, source: &Path) -> PathBuf {
        let stem = source.file_stem().unwrap_or_default();
        let parent = source.parent().unwrap_or(Path::new(""));
        let output_name = format!("{}.pdf", stem.to_string_lossy());
        Path::new(&self.config.output_dir).join(parent).join(output_name)
    }

    /// Remove temporary files produced by pdflatex in the given directory.
    fn clean_temp_files(&self, stem: &str, dir: &Path) {
        for ext in PDFLATEX_TEMP_EXTENSIONS {
            let path = dir.join(format!("{}{}", stem, ext));
            let _ = fs::remove_file(path);
        }
    }
}

impl ProductDiscovery for PdflatexProcessor {
    fn description(&self) -> &str {
        "Compile LaTeX documents using pdflatex"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.pdflatex.clone()];
        if self.config.qpdf {
            tools.push("qpdf".to_string());
        }
        tools
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }

        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for source in files {
            let output = self.output_path(&source);

            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(source);
            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, vec![output], crate::processors::names::PDFLATEX, hash.clone())?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let final_output = product.outputs.first()
            .context("pdflatex product has no output")?;

        // pdflatex writes output next to the input or in -output-directory
        // We use a temp directory for the build, then move the PDF to the final output location.
        let input_stem = input.file_stem()
            .context("pdflatex input has no file stem")?
            .to_string_lossy();

        // Ensure output directory exists
        if let Some(parent) = final_output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create pdflatex output directory: {}", parent.display()))?;
        }

        // Use the output directory as pdflatex's output-directory
        let build_dir = final_output.parent().unwrap_or(Path::new("."));

        // Run pdflatex N times
        for run in 0..self.config.runs {
            // Clean temp files between runs (not before first run)
            if run > 0 {
                self.clean_temp_files(&input_stem, build_dir);
            }

            let mut cmd = Command::new(&self.config.pdflatex);
            cmd.arg("-shell-escape");
            cmd.arg("-interaction=nonstopmode");
            cmd.arg("-halt-on-error");
            cmd.arg(format!("-output-directory={}", build_dir.display()));
            for arg in &self.config.args {
                cmd.arg(arg);
            }
            cmd.arg(input);

            let out = run_command(&mut cmd)?;
            check_command_output(&out, format_args!("pdflatex run {} of {}", run + 1, input.display()))?;
        }

        // Optional qpdf post-processing
        if self.config.qpdf {
            let pdf_in_build = build_dir.join(format!("{}.pdf", input_stem));
            let qpdf_tmp = build_dir.join(format!("{}.qpdf.pdf", input_stem));

            let mut cmd = Command::new("qpdf");
            cmd.arg("--deterministic-id");
            cmd.arg("--linearize");
            cmd.arg(&pdf_in_build);
            cmd.arg(&qpdf_tmp);

            let out = run_command(&mut cmd)?;
            check_command_output(&out, format_args!("qpdf {}", pdf_in_build.display()))?;

            // Replace original with linearized version
            fs::rename(&qpdf_tmp, &pdf_in_build)
                .with_context(|| format!("Failed to rename qpdf output: {}", qpdf_tmp.display()))?;
        }

        // Clean up temp files after final run
        self.clean_temp_files(&input_stem, build_dir);

        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::PDFLATEX, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
