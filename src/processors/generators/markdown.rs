use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command_capture, check_command_output};

impl_generator!(MarkdownProcessor, crate::config::MarkdownConfig,
    description: "Convert Markdown to HTML using markdown",
    name: crate::processors::names::MARKDOWN,
    discover: single_format, extension: "html",
    tool_field_extra: markdown_bin ["perl".to_string()]
);

impl MarkdownProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.markdown_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command_capture(&mut cmd)?;
        check_command_output(&out, format_args!("markdown {}", input.display()))?;

        fs::write(output, &out.stdout)
            .with_context(|| format!("Failed to write markdown output: {}", output.display()))?;

        Ok(())
    }
}
