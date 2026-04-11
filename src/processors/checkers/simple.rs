use std::path::Path;
use anyhow::Result;

use crate::config::{CheckerConfigWithCommand, SimpleCheckerParams};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, run_checker, execute_checker_batch,
    discover_checker_products, config_file_inputs};

/// A simple checker processor driven entirely by data.
/// Replaces the `simple_checker!` macro — all 29 trivial checkers use this struct
/// with different `SimpleCheckerParams`.
pub struct SimpleChecker {
    config: CheckerConfigWithCommand,
    params: SimpleCheckerParams,
}

impl SimpleChecker {
    pub fn new(config: CheckerConfigWithCommand, params: SimpleCheckerParams) -> Self {
        Self { config, params }
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let tool = &self.config.command;
        if self.params.prepend_args.is_empty() {
            run_checker(tool, self.params.subcommand, &self.config.args, files)
        } else {
            let mut combined_args: Vec<String> = self.params.prepend_args.iter().map(|s| s.to_string()).collect();
            combined_args.extend_from_slice(&self.config.args);
            run_checker(tool, self.params.subcommand, &combined_args, files)
        }
    }
}

impl ProductDiscovery for SimpleChecker {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        self.params.description
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.command.clone()];
        for t in self.params.extra_tools {
            tools.push(t.to_string());
        }
        tools
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let mut dep_inputs = self.config.dep_inputs.clone();
        for ai in &self.config.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        discover_checker_products(
            graph, &self.config.scan, file_index, &dep_inputs, &self.config, instance_name,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }


    fn supports_batch(&self) -> bool {
        self.config.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(products, |files| self.check_files(files))
    }
}

// --- Plugin registrations ---

inventory::submit! { &crate::registry::simple_checker_plugin("ruff", SimpleCheckerParams { description: "Lint Python files with ruff", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("pylint", SimpleCheckerParams { description: "Lint Python files with pylint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("pytest", SimpleCheckerParams { description: "Run Python tests with pytest", subcommand: None, prepend_args: &[], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("black", SimpleCheckerParams { description: "Check Python formatting with black", subcommand: None, prepend_args: &["--check"], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("doctest", SimpleCheckerParams { description: "Run Python doctests", subcommand: None, prepend_args: &["-m", "doctest"], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("mypy", SimpleCheckerParams { description: "Type-check Python files with mypy", subcommand: None, prepend_args: &[], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("pyrefly", SimpleCheckerParams { description: "Type-check Python files with pyrefly", subcommand: Some("check"), prepend_args: &["--disable-project-excludes-heuristics"], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("rumdl", SimpleCheckerParams { description: "Lint Markdown files using rumdl", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("yamllint", SimpleCheckerParams { description: "Lint YAML files with yamllint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("jq", SimpleCheckerParams { description: "Validate JSON files with jq", subcommand: None, prepend_args: &["empty"], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("jsonlint", SimpleCheckerParams { description: "Lint JSON files with jsonlint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("taplo", SimpleCheckerParams { description: "Check TOML files with taplo", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("eslint", SimpleCheckerParams { description: "Lint JavaScript/TypeScript files with eslint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("jshint", SimpleCheckerParams { description: "Lint JavaScript files with jshint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("htmlhint", SimpleCheckerParams { description: "Lint HTML files with htmlhint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("stylelint", SimpleCheckerParams { description: "Lint CSS/SCSS files with stylelint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("checkstyle", SimpleCheckerParams { description: "Check Java code style with checkstyle", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("cmake", SimpleCheckerParams { description: "Lint CMakeLists.txt files with cmake --lint", subcommand: Some("--lint"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("hadolint", SimpleCheckerParams { description: "Lint Dockerfiles with hadolint", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("htmllint", SimpleCheckerParams { description: "Lint HTML files with htmllint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("jslint", SimpleCheckerParams { description: "Lint JavaScript files with jslint", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("perlcritic", SimpleCheckerParams { description: "Analyze Perl code with perlcritic", subcommand: None, prepend_args: &[], extra_tools: &["perl"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("php_lint", SimpleCheckerParams { description: "Check PHP syntax with php -l", subcommand: Some("-l"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("slidev", SimpleCheckerParams { description: "Build Slidev presentations", subcommand: Some("build"), prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("standard", SimpleCheckerParams { description: "Check JavaScript style with standard", subcommand: None, prepend_args: &[], extra_tools: &["node"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("svglint", SimpleCheckerParams { description: "Lint SVG files with svglint", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("tidy", SimpleCheckerParams { description: "Validate HTML files with tidy", subcommand: Some("-errors"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("xmllint", SimpleCheckerParams { description: "Validate XML files with xmllint", subcommand: Some("--noout"), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("yq", SimpleCheckerParams { description: "Validate YAML files with yq", subcommand: Some("."), prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("cppcheck", SimpleCheckerParams { description: "Run cppcheck static analysis on C/C++ source files", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("cpplint", SimpleCheckerParams { description: "Run cpplint (Google C++ style checker) on C/C++ source files", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("checkpatch", SimpleCheckerParams { description: "Run kernel checkpatch.pl on C source files", subcommand: None, prepend_args: &["--no-tree", "-f"], extra_tools: &["perl"] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("shellcheck", SimpleCheckerParams { description: "Lint shell scripts using shellcheck", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
inventory::submit! { &crate::registry::simple_checker_plugin("luacheck", SimpleCheckerParams { description: "Lint Lua scripts using luacheck", subcommand: None, prepend_args: &[], extra_tools: &[] }) as &dyn crate::registry::RegistryOps }
