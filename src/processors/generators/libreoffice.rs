use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(LibreofficeProcessor, crate::config::LibreofficeConfig,
    description: "Convert LibreOffice documents to PDF/PPTX",
    name: crate::processors::names::LIBREOFFICE,
    discover: multi_format, formats_field: formats,
    tool_field_extra: libreoffice_bin ["flock".into()]
);

impl LibreofficeProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("libreoffice output has no extension")?
            .to_string_lossy();

        let output_dir = output.parent()
            .context("libreoffice output has no parent directory")?;

        fs::create_dir_all(output_dir)
            .with_context(|| format!("Failed to create libreoffice output directory: {}", output_dir.display()))?;

        // Use flock to serialize LibreOffice invocations (it can't run multiple instances)
        let mut cmd = Command::new("flock");
        cmd.arg("/tmp/rsconstruct_libreoffice");
        cmd.arg(&self.config.libreoffice_bin);
        cmd.arg("--headless");
        cmd.arg("--convert-to").arg(format.as_ref());
        cmd.arg("--outdir").arg(output_dir);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("libreoffice {}", input.display()))
    }
}
