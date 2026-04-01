use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(PandocProcessor, crate::config::PandocConfig,
    description: "Convert documents using pandoc",
    name: crate::processors::names::PANDOC,
    discover: multi_format, formats_field: formats,
    tool_field: pandoc
);

/// Path to the deterministic PDF header file (created once per process).
static PDF_HEADER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Get or create a temporary TeX header file that suppresses the random PDF trailer ID.
fn deterministic_pdf_header() -> &'static std::path::PathBuf {
    PDF_HEADER.get_or_init(|| {
        let dir = std::env::temp_dir().join("rsconstruct-pandoc");
        std::fs::create_dir_all(&dir).expect("Failed to create pandoc temp dir");
        let path = dir.join("deterministic.tex");
        let mut f = std::fs::File::create(&path).expect("Failed to create pandoc header file");
        f.write_all(b"\\pdftrailerid{}\n").expect("Failed to write pandoc header file");
        path
    })
}

impl PandocProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("pandoc output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.pandoc);
        // Deterministic output: fixed timestamps and no random PDF IDs
        cmd.env("SOURCE_DATE_EPOCH", "0");
        cmd.arg("--from").arg(&self.config.from);
        cmd.arg("--to").arg(format.as_ref());
        // For PDF output, suppress the random trailer ID
        if format.as_ref() == "pdf" {
            cmd.arg("--include-in-header").arg(deterministic_pdf_header());
        }
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);
        cmd.arg("-o").arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("pandoc {}", input.display()))
    }
}
