//! ijinja2 generator: Jinja2 templates rendered in-process with minijinja.
//!
//! The Rust alternative to the `jinja2` generator, with the same contract:
//! every template under `src_dirs` whose name ends in a `src_extensions`
//! entry is rendered to the same path with the extension removed, the
//! project root is the loader root (so `{% include %}` and `{% extends %}`
//! take paths relative to it), and the environment variables are the
//! context. The engine is configured to match a plain Jinja2
//! `Environment`: no autoescaping, the trailing newline dropped, undefined
//! names rendering as nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::Processor;

use super::TemplateItem;

/// No custom fields. Unused `StandardConfig` fields: command, formats,
/// `output_dir`.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Ijinja2Config {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

/// A Jinja2-like environment: the project root as the loader root, no
/// autoescaping (Jinja2's default), the trailing newline dropped, line
/// endings normalised to `\n` as Jinja2's lexer does, and the Python
/// string and dict methods templates commonly call (`.items()`,
/// `.lower()`, `.startswith()`, ...) provided by minijinja-contrib.
fn environment() -> Result<minijinja::Environment<'static>> {
    let mut env = minijinja::Environment::new();
    env.set_loader(|name| {
        let path = Path::new(".").join(name);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text.replace("\r\n", "\n").replace('\r', "\n"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(minijinja::Error::new(
                minijinja::ErrorKind::TemplateNotFound,
                format!("could not read {}: {e}", path.display()),
            )),
        }
    });
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    // Jinja2 filters whose argument lists minijinja does not cover.
    env.add_filter("truncate", filters::truncate);
    env.add_filter("replace", filters::replace);
    env.add_filter("round", filters::round);
    env.add_filter("int", filters::int);
    env.add_filter("sum", filters::sum);
    env.add_filter("center", filters::center);
    env.add_filter("forceescape", filters::forceescape);
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    let syntax = minijinja::syntax::SyntaxConfig::builder()
        .keep_trailing_newline(false)
        .build()
        .context("Failed to configure the Jinja2 syntax")?;
    env.set_syntax(syntax);
    Ok(env)
}

fn render(item: &TemplateItem) -> Result<()> {
    crate::processor::ensure_output_dir(&item.output_path)?;
    let env = environment()?;
    let name = item.source_path.to_string_lossy();
    let template = env
        .get_template(&name)
        .with_context(|| format!("Failed to load template {}", item.source_path.display()))?;
    let vars: BTreeMap<String, String> = std::env::vars().collect();
    let rendered = template
        .render(minijinja::Value::from_pairs(vars))
        .with_context(|| format!("Failed to render template {}", item.source_path.display()))?;
    fs::write(&item.output_path, rendered)
        .with_context(|| format!("Failed to write output: {}", item.output_path.display()))
}

/// Jinja2's versions of filters minijinja has with a shorter argument
/// list, or not at all: positional arguments as Jinja2 takes them.
mod filters {
    use minijinja::value::{Kwargs, Value, ValueKind};
    use minijinja::{Error, ErrorKind};

    /// `do_truncate(s, length=255, killwords=False, end='...', leeway=5)`;
    /// `leeway` is keyword-only here.
    pub fn truncate(
        value: Value,
        length: Option<usize>,
        killwords: Option<bool>,
        end: Option<String>,
        kwargs: Kwargs,
    ) -> Result<Value, Error> {
        let s = value.as_str().unwrap_or_default().to_string();
        let length = match length {
            Some(l) => l,
            None => kwargs.get::<Option<usize>>("length")?.unwrap_or(255),
        };
        let killwords = match killwords {
            Some(k) => k,
            None => kwargs.get::<Option<bool>>("killwords")?.unwrap_or(false),
        };
        let end = match end {
            Some(e) => e,
            None => kwargs
                .get::<Option<String>>("end")?
                .unwrap_or_else(|| "...".to_string()),
        };
        let leeway = kwargs.get::<Option<usize>>("leeway")?.unwrap_or(5);
        kwargs.assert_all_used()?;
        if length < end.chars().count() {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                format!("expected length >= {}, got {length}", end.chars().count()),
            ));
        }
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= length + leeway {
            return Ok(Value::from(s));
        }
        let cut: String = chars[..length - end.chars().count()].iter().collect();
        if killwords {
            return Ok(Value::from(format!("{cut}{end}")));
        }
        let kept = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
        Ok(Value::from(format!("{kept}{end}")))
    }

    /// `do_replace(s, old, new, count=None)`.
    pub fn replace(value: Value, old: String, new: String, count: Option<usize>) -> Value {
        let s = value.to_string();
        Value::from(match count {
            Some(n) => s.replacen(&old, &new, n),
            None => s.replace(&old, &new),
        })
    }

    /// `do_round(value, precision=0, method='common')`.
    pub fn round(
        value: Value,
        precision: Option<i32>,
        method: Option<String>,
    ) -> Result<Value, Error> {
        let x = f64::try_from(value.clone()).map_err(|_| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("cannot round {}", value.kind()),
            )
        })?;
        let precision = precision.unwrap_or(0);
        let factor = 10f64.powi(precision);
        let scaled = x * factor;
        let rounded = match method.as_deref().unwrap_or("common") {
            "common" => scaled.round(),
            "ceil" => scaled.ceil(),
            "floor" => scaled.floor(),
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidOperation,
                    format!("{other:?} is not a valid rounding method"),
                ));
            }
        };
        Ok(Value::from(rounded / factor))
    }

    /// `do_int(value, default=0, base=10)`.
    pub fn int(value: Value, default: Option<i64>, base: Option<u32>) -> Value {
        let default = default.unwrap_or(0);
        let base = base.unwrap_or(10);
        if let Some(s) = value.as_str() {
            let t = s.trim();
            let (negative, digits) = match t.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, t.strip_prefix('+').unwrap_or(t)),
            };
            let digits = match base {
                16 => digits
                    .strip_prefix("0x")
                    .or_else(|| digits.strip_prefix("0X"))
                    .unwrap_or(digits),
                8 => digits
                    .strip_prefix("0o")
                    .or_else(|| digits.strip_prefix("0O"))
                    .unwrap_or(digits),
                2 => digits
                    .strip_prefix("0b")
                    .or_else(|| digits.strip_prefix("0B"))
                    .unwrap_or(digits),
                _ => digits,
            };
            let parsed = i64::from_str_radix(digits, base).ok().or_else(|| {
                if base == 10 {
                    t.parse::<f64>().ok().map(|f| f as i64)
                } else {
                    None
                }
            });
            return Value::from(match parsed {
                Some(n) if negative && base != 10 => -n,
                Some(n) => n,
                None => default,
            });
        }
        match value.kind() {
            ValueKind::Number => Value::from(f64::try_from(value).map_or(0, |f| f as i64)),
            ValueKind::Bool => Value::from(i64::from(value.is_true())),
            _ => Value::from(default),
        }
    }

    /// `sync_do_sum(iterable, attribute=None, start=0)` over numbers.
    pub fn sum(
        values: Value,
        attribute: Option<String>,
        start: Option<Value>,
        kwargs: Kwargs,
    ) -> Result<Value, Error> {
        let attribute = match attribute {
            Some(a) => Some(a),
            None => kwargs.get::<Option<String>>("attribute")?,
        };
        let start = match start {
            Some(s) => Some(s),
            None => kwargs.get::<Option<Value>>("start")?,
        };
        kwargs.assert_all_used()?;
        let mut acc_int: Option<i64> = Some(0);
        let mut acc = 0f64;
        let mut add = |v: &Value| -> Result<(), Error> {
            let f = f64::try_from(v.clone()).map_err(|_| {
                Error::new(
                    ErrorKind::InvalidOperation,
                    format!("cannot sum {}", v.kind()),
                )
            })?;
            acc += f;
            acc_int = match (acc_int, i64::try_from(v.clone())) {
                (Some(a), Ok(i)) => a.checked_add(i),
                _ => None,
            };
            Ok(())
        };
        if let Some(s) = &start {
            add(s)?;
        }
        for item in values.try_iter()? {
            let item = match &attribute {
                Some(a) => item.get_attr(a)?,
                None => item,
            };
            add(&item)?;
        }
        Ok(match acc_int {
            Some(i) => Value::from(i),
            None => Value::from(acc),
        })
    }

    /// `do_center(value, width=80)`.
    pub fn center(value: Value, width: Option<usize>) -> Value {
        let s = value.to_string();
        let width = width.unwrap_or(80);
        let len = s.chars().count();
        if len >= width {
            return Value::from(s);
        }
        // Python's str.center: the extra space goes to the right when odd... except
        // that it alternates by width parity; `format!("{:^}")` matches str.center
        // only for even padding, so do it by hand the way CPython does.
        let pad = width - len;
        let left = pad / 2 + (pad & width & 1);
        let right = pad - left;
        Value::from(format!("{}{s}{}", " ".repeat(left), " ".repeat(right)))
    }

    /// `do_forceescape`: escape even values marked safe.
    pub fn forceescape(value: Value) -> Value {
        let s = value.to_string();
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&#34;"),
                '\'' => out.push_str("&#39;"),
                _ => out.push(c),
            }
        }
        Value::from_safe_string(out)
    }
}

pub struct Ijinja2Processor {
    config: Ijinja2Config,
}

impl Ijinja2Processor {
    pub const fn new(config: Ijinja2Config) -> Self {
        Self { config }
    }
}

impl Processor for Ijinja2Processor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned template (`find_templates`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !super::find_templates(&self.config.standard, file_index).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let items = super::find_templates(&self.config.standard, file_index);
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                instance_name,
                Some(output_config_hash(
                    &self.config,
                    &crate::config::checksum_fields_of(instance_name),
                )),
            )?;
        }
        Ok(())
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        render(&item)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(Ijinja2Processor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "ijinja2",
        processor_type: crate::processor::ProcessorType::Generator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["command"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".j2"], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<Ijinja2Config>,
        keywords: &["template", "generator", "jinja", "jinja2", "rust"],
        description: "Render Jinja2 templates into output files (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: None,
    }
}
