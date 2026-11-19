//! pytest checker — a {`SimpleChecker`} with pytest's own discovery.

use anyhow::Result;

use crate::config::{CheckerConfig, ExtraTool, SimpleCheckerParams, StandardConfig};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Discovery, Processor, SimpleChecker};

/// The file pytest loads from a test's directory and every directory above
/// it, for fixtures and hooks.
const CONFTEST: &str = "conftest.py";

/// Runs like any simple checker; discovers like pytest. A `conftest.py` is
/// not a test: it is an input of every test in or below its directory.
///
/// Scanned like the tests around it, a `conftest.py` was a product of its
/// own (`pytest tests/conftest.py` collects nothing and exits 5), and no
/// test depended on it, so a fixture changed under `tests/` left every
/// cached test result in place. Only a `conftest.py` at the project root
/// was an input, through `dep_auto`.
struct PytestProcessor {
    config: CheckerConfig,
    checker: SimpleChecker,
}

impl Processor for PytestProcessor {
    fn scan_config(&self) -> &StandardConfig {
        self.checker.scan_config()
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.checker.auto_detect(file_index)
    }

    fn required_tools(&self) -> Vec<String> {
        self.checker.required_tools()
    }

    fn discovery(&self) -> Discovery {
        Discovery::PerFile
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let standard = &self.config.standard;
        let hash = Some(crate::config::output_config_hash(
            &self.config,
            <CheckerConfig as crate::config::KnownFields>::checksum_fields(),
        ));
        let shared =
            crate::processor::resolve_dep_inputs(&standard.dep_inputs, &standard.dep_auto)?;
        for file in file_index.scan(standard, true) {
            if file.file_name().is_some_and(|name| name == CONFTEST) {
                continue;
            }
            let mut inputs = vec![file.clone()];
            inputs.extend_from_slice(&shared);
            for dir in file.ancestors().skip(1) {
                let conftest = dir.join(CONFTEST);
                if conftest.is_file() && !inputs.contains(&conftest) {
                    inputs.push(conftest);
                }
            }
            graph.add_product(inputs, vec![], instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.checker.execute(ctx, product)
    }

    fn execute_batch(
        &self,
        ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        self.checker.execute_batch(ctx, products)
    }
}

fn create_pytest(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg: CheckerConfig| {
        Box::new(PytestProcessor {
            config: cfg.clone(),
            checker: SimpleChecker::new(
                cfg,
                SimpleCheckerParams {
                    description: "Run Python tests using pytest",
                    subcommand: None,
                    prepend_args: &[],
                    extra_tools: &[ExtraTool::Python],
                },
            ),
        })
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "pytest", processor_type: crate::processor::ProcessorType::Checker, create: create_pytest,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "pytest", dep_auto: &["conftest.py", "pytest.ini", "pyproject.toml"], hash_python_env: true, ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["python", "tester", "testing", "py", "pip"],
    description: "Run Python tests using pytest",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    sandboxable: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "pytest", runtime: "python",
    project: Some(crate::tools::InstallMethod::new("pip", "pytest")),
    machine: Some(crate::tools::InstallMethod::new("pip", "pytest")),
    alternatives: &[], prefer: None, sandbox_writable: &[".pytest_cache", "**/__pycache__"], needs_network: false,
} }
