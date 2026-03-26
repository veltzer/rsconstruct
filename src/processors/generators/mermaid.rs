use anyhow::Result;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(MermaidProcessor, crate::config::MermaidConfig,
    description: "Convert Mermaid diagrams to PNG/SVG/PDF",
    name: crate::processors::names::MERMAID,
    discover: multi_format, formats_field: formats,
    tool_field_extra: mmdc_bin ["node".to_string()]
);

impl MermaidProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.mmdc_bin);
        cmd.arg("-i").arg(input);
        cmd.arg("-o").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("mmdc {}", input.display()))
    }
}
