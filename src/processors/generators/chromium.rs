use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(ChromiumProcessor, crate::config::ChromiumConfig,
    description: "Convert HTML to PDF using headless Chromium",
    name: crate::processors::names::CHROMIUM,
    discover: single_format, extension: "pdf",
    tool_field: chromium_bin
);

impl ChromiumProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        // Convert the input path to an absolute file:// URL for Chromium
        let abs_input = fs::canonicalize(input)
            .with_context(|| format!("Failed to resolve absolute path for: {}", input.display()))?;
        let input_url = format!("file://{}", abs_input.display());

        let mut cmd = Command::new(&self.config.chromium_bin);
        cmd.arg("--headless");
        cmd.arg("--disable-gpu");
        cmd.arg("--no-sandbox");
        cmd.arg(format!("--print-to-pdf={}", output.display()));
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(&input_url);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("chromium {}", input.display()))
    }
}
