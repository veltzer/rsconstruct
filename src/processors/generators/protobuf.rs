use anyhow::Result;
use std::process::Command;

use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

impl_generator!(ProtobufProcessor, crate::config::ProtobufConfig,
    description: "Compile Protocol Buffer files",
    name: crate::processors::names::PROTOBUF,
    discover: single_format, extension: "pb.cc",
    tool_field: protoc_bin
);

impl ProtobufProcessor {
    fn execute_product(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();
        let output_dir = output.parent().unwrap_or(std::path::Path::new("."));

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.protoc_bin);
        // Set the proto path to the directory containing the input file
        if let Some(parent) = input.parent() {
            cmd.arg(format!("--proto_path={}", parent.display()));
        }
        cmd.arg(format!("--cpp_out={}", output_dir.display()));
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("protoc {}", input.display()))
    }
}
