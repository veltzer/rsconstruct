use anyhow::{Context, Result};
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(PandocProcessor, crate::config::PandocConfig,
    description: "Convert documents using pandoc",
    name: crate::processors::names::PANDOC,
    discover: multi_format, formats_field: formats,
    tool_field: pandoc
);

impl PandocProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("pandoc output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.pandoc);
        // Deterministic output: fixed timestamps
        cmd.env("SOURCE_DATE_EPOCH", "0");
        cmd.arg("--from").arg(&self.config.from);
        cmd.arg("--to").arg(format.as_ref());
        // For PDF output, suppress the random trailer ID
        if format.as_ref() == "pdf" {
            cmd.arg("-V").arg(r"header-includes=\pdftrailerid{}");
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
