use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(A2xProcessor, crate::config::A2xConfig,
    description: "Convert AsciiDoc to PDF using a2x",
    name: crate::processors::names::A2X,
    discover: single_format, extension: "pdf",
    tool_field_extra: a2x ["python3".to_string()]
);

impl A2xProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.a2x);
        cmd.arg("-f").arg(&self.config.format);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("a2x {}", input.display()))?;

        // a2x generates the PDF next to the input file — move it to the output path
        let stem = input.file_stem()
            .context("a2x input has no file stem")?;
        let generated = input.with_file_name(format!("{}.pdf", stem.to_string_lossy()));

        if generated != *output && generated.exists() {
            fs::rename(&generated, output)
                .with_context(|| format!("Failed to move a2x output from {} to {}", generated.display(), output.display()))?;
        }

        Ok(())
    }
}
