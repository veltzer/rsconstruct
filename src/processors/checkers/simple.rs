use std::path::Path;
use anyhow::Result;

use crate::config::{CheckerConfigWithCommand, SimpleCheckerParams};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{Processor, run_checker, execute_checker_batch,
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
        let tool = &self.config.standard.command;
        if self.params.prepend_args.is_empty() {
            run_checker(tool, self.params.subcommand, &self.config.standard.args, files)
        } else {
            let mut combined_args: Vec<String> = self.params.prepend_args.iter().map(|s| s.to_string()).collect();
            combined_args.extend_from_slice(&self.config.standard.args);
            run_checker(tool, self.params.subcommand, &combined_args, files)
        }
    }
}

impl Processor for SimpleChecker {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        self.params.description
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.standard.command.clone()];
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
        let mut dep_inputs = self.config.standard.dep_inputs.clone();
        for ai in &self.config.standard.dep_auto {
            dep_inputs.extend(config_file_inputs(ai));
        }
        discover_checker_products(
            graph, &self.config.standard, file_index, &dep_inputs, &self.config, instance_name,
        )
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }


    fn supports_batch(&self) -> bool {
        self.config.standard.batch
    }

    fn execute_batch(&self, products: &[&Product]) -> Vec<Result<()>> {
        execute_checker_batch(products, |files| self.check_files(files))
    }
}


// --- Plugin registrations ---


// --- Plugin registrations ---

fn create_ruff(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint and format Python files using ruff", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "ruff", processor_type: crate::processors::ProcessorType::Checker, create: create_ruff,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_pylint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint Python files using pylint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "pylint", processor_type: crate::processors::ProcessorType::Checker, create: create_pylint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_pytest(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Run Python tests using pytest", subcommand: None, prepend_args: &[], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "pytest", processor_type: crate::processors::ProcessorType::Checker, create: create_pytest,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_black(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check Python code formatting using black", subcommand: None, prepend_args: &["--check"], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "black", processor_type: crate::processors::ProcessorType::Checker, create: create_black,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_doctest(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Run Python doctests", subcommand: None, prepend_args: &["-m", "doctest"], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "doctest", processor_type: crate::processors::ProcessorType::Checker, create: create_doctest,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_mypy(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Type-check Python files using mypy", subcommand: None, prepend_args: &[], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "mypy", processor_type: crate::processors::ProcessorType::Checker, create: create_mypy,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_pyrefly(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Type-check Python files using pyrefly", subcommand: Some("check"), prepend_args: &["--disable-project-excludes-heuristics"], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "pyrefly", processor_type: crate::processors::ProcessorType::Checker, create: create_pyrefly,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_rumdl(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint Markdown files using rumdl", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "rumdl", processor_type: crate::processors::ProcessorType::Checker, create: create_rumdl,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_yamllint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint YAML files using yamllint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "yamllint", processor_type: crate::processors::ProcessorType::Checker, create: create_yamllint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_jq(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Validate JSON files using jq", subcommand: None, prepend_args: &["empty"], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "jq", processor_type: crate::processors::ProcessorType::Checker, create: create_jq,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_jsonlint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint JSON files using jsonlint", subcommand: None, prepend_args: &[], extra_tools: &["python3"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "jsonlint", processor_type: crate::processors::ProcessorType::Checker, create: create_jsonlint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_taplo(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check TOML files using taplo", subcommand: Some("check"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "taplo", processor_type: crate::processors::ProcessorType::Checker, create: create_taplo,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_eslint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint JavaScript/TypeScript files using ESLint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "eslint", processor_type: crate::processors::ProcessorType::Checker, create: create_eslint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_jshint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint JavaScript files using JSHint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "jshint", processor_type: crate::processors::ProcessorType::Checker, create: create_jshint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_htmlhint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint HTML files using HTMLHint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "htmlhint", processor_type: crate::processors::ProcessorType::Checker, create: create_htmlhint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_stylelint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint CSS/SCSS files using stylelint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "stylelint", processor_type: crate::processors::ProcessorType::Checker, create: create_stylelint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_checkstyle(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check Java code style using Checkstyle", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "checkstyle", processor_type: crate::processors::ProcessorType::Checker, create: create_checkstyle,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_cmake(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint CMake files using cmake --lint", subcommand: Some("--lint"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "cmake", processor_type: crate::processors::ProcessorType::Checker, create: create_cmake,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_hadolint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint Dockerfiles using hadolint", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "hadolint", processor_type: crate::processors::ProcessorType::Checker, create: create_hadolint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_htmllint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint HTML files using htmllint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "htmllint", processor_type: crate::processors::ProcessorType::Checker, create: create_htmllint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_jslint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint JavaScript files using JSLint", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "jslint", processor_type: crate::processors::ProcessorType::Checker, create: create_jslint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_perlcritic(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Analyze Perl code using perlcritic", subcommand: None, prepend_args: &[], extra_tools: &["perl"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "perlcritic", processor_type: crate::processors::ProcessorType::Checker, create: create_perlcritic,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_php_lint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check PHP syntax using php -l", subcommand: Some("-l"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "php_lint", processor_type: crate::processors::ProcessorType::Checker, create: create_php_lint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_slidev(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Build Slidev presentations", subcommand: Some("build"), prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "slidev", processor_type: crate::processors::ProcessorType::Checker, create: create_slidev,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_standard(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check JavaScript style using standard", subcommand: None, prepend_args: &[], extra_tools: &["node"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "standard", processor_type: crate::processors::ProcessorType::Checker, create: create_standard,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_svglint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint SVG files using svglint", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "svglint", processor_type: crate::processors::ProcessorType::Checker, create: create_svglint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_tidy(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Validate HTML files using tidy", subcommand: Some("-errors"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "tidy", processor_type: crate::processors::ProcessorType::Checker, create: create_tidy,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_xmllint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Validate XML files using xmllint", subcommand: Some("--noout"), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "xmllint", processor_type: crate::processors::ProcessorType::Checker, create: create_xmllint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_yq(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Validate YAML files using yq", subcommand: Some("."), prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "yq", processor_type: crate::processors::ProcessorType::Checker, create: create_yq,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_cppcheck(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Static analysis for C/C++ using cppcheck", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "cppcheck", processor_type: crate::processors::ProcessorType::Checker, create: create_cppcheck,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_cpplint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint C/C++ files using cpplint", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "cpplint", processor_type: crate::processors::ProcessorType::Checker, create: create_cpplint,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_checkpatch(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Check kernel patches using checkpatch.pl", subcommand: None, prepend_args: &["--no-tree", "-f"], extra_tools: &["perl"] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "checkpatch", processor_type: crate::processors::ProcessorType::Checker, create: create_checkpatch,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_shellcheck(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint shell scripts using shellcheck", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "shellcheck", processor_type: crate::processors::ProcessorType::Checker, create: create_shellcheck,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_luacheck(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Lint Lua files using luacheck", subcommand: None, prepend_args: &[], extra_tools: &[] })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "luacheck", processor_type: crate::processors::ProcessorType::Checker, create: create_luacheck,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

