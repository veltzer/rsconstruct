//! ixmllint checker: `xmllint --noout [--schema x.xsd]`, in-process.
//!
//! The engine is `crate::engines::xmllint`: well-formedness and namespace checks that
//! report what xmllint reports, and an XSD validator for the schema
//! constructs in use. This file is the processor around it: which schema is
//! read, what fails a product, how problems are reported.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::xmllint::{self, Schema};
use crate::graph::Product;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct IxmllintConfig {
    /// An XSD schema every file must validate against, like
    /// `xmllint --schema`; a path relative to the project root. Empty means
    /// well-formedness only. The schema is read once per build.
    #[serde(default)]
    pub schema: String,
    /// Fail on namespace errors and parser warnings too. Off, they are
    /// printed and the product passes, as `xmllint` exits 0 on them.
    #[serde(default)]
    pub strict: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct IxmllintProcessor {
    config: IxmllintConfig,
    /// The compiled schema, read once per build. The error is kept as text
    /// so every product reports the same schema failure.
    schema: OnceLock<Result<Option<Schema>, String>>,
}

impl IxmllintProcessor {
    pub const fn new(config: IxmllintConfig) -> Self {
        Self {
            config,
            schema: OnceLock::new(),
        }
    }

    fn schema(&self) -> Result<Option<&Schema>> {
        let loaded = self.schema.get_or_init(|| {
            if self.config.schema.is_empty() {
                return Ok(None);
            }
            Schema::from_file(Path::new(&self.config.schema))
                .map(Some)
                .map_err(|e| format!("{e:#}"))
        });
        match loaded {
            Ok(schema) => Ok(schema.as_ref()),
            Err(msg) => bail!("{msg}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let schema = self.schema()?;
        let mut failures = Vec::new();

        for file in files {
            let bytes = std::fs::read(file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            let document = xmllint::parse(&bytes);
            let well_formed = document.is_well_formed();
            let mut problems = document.problems;
            if let (Some(schema), Some(root), true) = (schema, document.root.as_ref(), well_formed)
            {
                let found = xmllint::validate(root, schema).with_context(|| {
                    format!(
                        "Failed to validate {} against {}",
                        file.display(),
                        self.config.schema
                    )
                })?;
                problems.extend(found);
            }
            for problem in problems {
                let line = format!("{}:{problem}", file.display());
                if problem.kind.is_fatal() || self.config.strict {
                    failures.push(line);
                } else {
                    crate::output::warn(&line);
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }
}

impl crate::processor::Processor for IxmllintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    /// The schema decides pass/fail just as much as the file itself, so it
    /// and every schema it `include`s are inputs of every product: an edit
    /// to any of them invalidates cached passes without a `dep_inputs`
    /// entry. A named schema must exist.
    fn extra_dep_inputs(&self) -> Result<Vec<String>> {
        let Some(schema) =
            crate::processor::named_config_file("ixmllint", "schema", &self.config.schema, "")?
        else {
            return Ok(Vec::new());
        };
        let mut files = Vec::new();
        collect_schema_files(Path::new(&schema), &mut files)?;
        Ok(files)
    }

    /// One product per scanned file (the default `discover`), plus the
    /// schema files read from disk.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_files(&[file]))
    }
}

/// The XML Schema namespace, in which `xs:include` lives.
const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";

/// `path` and the schemas it `include`s, transitively, each once, in the
/// order the validator's loader reads them: a `schemaLocation` is relative
/// to the including file's directory. An `xs:import` is not followed, as
/// the loader rejects it. A schema that cannot be read is an error naming
/// it; what is wrong inside one is the loader's to report.
fn collect_schema_files(path: &Path, files: &mut Vec<String>) -> Result<()> {
    let name = path.display().to_string();
    if files.contains(&name) {
        return Ok(());
    }
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read XSD schema {}", path.display()))?;
    files.push(name);
    let Some(root) = xmllint::parse(&bytes).root else {
        return Ok(());
    };
    for child in &root.children {
        let xmllint::Child::Element(element) = child else {
            continue;
        };
        if element.namespace.as_deref() != Some(XSD_NS) || element.local != "include" {
            continue;
        }
        let Some(location) = element.attribute(None, "schemaLocation") else {
            continue;
        };
        let include_path = path.parent().map_or_else(
            || Path::new(&location.value).to_path_buf(),
            |dir| dir.join(&location.value),
        );
        collect_schema_files(&include_path, files)?;
    }
    Ok(())
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IxmllintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.xmllint", by: "processor.checker.ixmllint" }
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "ixmllint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IxmllintConfig>,
        fields: &[
            crate::config::FieldSpec { name: "schema", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "XSD schema to validate every file against, like xmllint --schema; relative to the project root. Empty: well-formedness only" },
            crate::config::FieldSpec { name: "strict", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Fail on namespace errors and parser warnings too (default: they are printed and the file passes, as xmllint exits 0 on them)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".xml", ".svg"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["xml", "svg", "linter", "validator", "xsd", "xmllint", "rust"],
        description: "Check XML well-formedness and validate against an XSD schema (in-process)",
        is_builtin: true,
        is_rust: true,
        supports_batch: true,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
