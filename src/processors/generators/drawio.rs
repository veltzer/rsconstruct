use anyhow::{Context, Result};
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(DrawioProcessor, crate::config::DrawioConfig,
    description: "Convert Draw.io diagrams to PNG/SVG/PDF",
    name: crate::processors::names::DRAWIO,
    discover: multi_format, formats_field: formats,
    tool_field: drawio_bin
);

impl DrawioProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("drawio output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.drawio_bin);
        cmd.arg("--export");
        cmd.arg("--format").arg(format.as_ref());
        cmd.arg("--output").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("drawio {}", input.display()))
    }
}
