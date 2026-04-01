use anyhow::Result;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(SassProcessor, crate::config::SassConfig,
    description: "Compile SCSS/SASS files to CSS",
    name: crate::processors::names::SASS,
    discover: single_format, extension: "css",
    tool_field: sass_bin
);

impl SassProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.sass_bin);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input).arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("sass {}", input.display()))
    }
}
