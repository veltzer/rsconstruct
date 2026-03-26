use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command_capture, check_command_output};

impl_generator!(ObjdumpProcessor, crate::config::ObjdumpConfig,
    description: "Disassemble binaries using objdump",
    name: crate::processors::names::OBJDUMP,
    discover: single_format, extension: "dis",
    tools: ["objdump".to_string()]
);

impl ObjdumpProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new("objdump");
        cmd.arg("--disassemble").arg("--source");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command_capture(&mut cmd)?;
        check_command_output(&out, format_args!("objdump {}", input.display()))?;

        fs::write(output, &out.stdout)
            .with_context(|| format!("Failed to write objdump output: {}", output.display()))?;

        Ok(())
    }
}
