use anyhow::{Context, Result};
use chrono::Datelike;
use mlua::LuaSerdeExt;
use mlua::prelude::*;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use tera::{Context as TeraContext, Function, Tera, Value as TeraValue, to_value};

use crate::config::{TeraConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, run_command_capture};

use super::TemplateItem;

/// Wrapper around a `&BuildContext` reference that can be stored in Tera function structs.
/// Tera's `Function` trait requires `Send + Sync + 'static`, so we cannot use a borrow.
/// Safety: the pointer is only dereferenced during `render_template`, which holds the
/// original `&BuildContext` reference for the entire duration of the Tera render call.
#[derive(Clone, Copy)]
struct CtxPtr(*const crate::build_context::BuildContext);

// SAFETY: BuildContext is Sync + Send; the pointer is only live while render_template runs.
unsafe impl Send for CtxPtr {}
unsafe impl Sync for CtxPtr {}

impl CtxPtr {
    fn get(&self) -> &crate::build_context::BuildContext {
        // SAFETY: caller guarantees the BuildContext outlives all uses of CtxPtr.
        unsafe { &*self.0 }
    }
}

/// Render a template item and write to output
fn render_template(ctx: &crate::build_context::BuildContext, item: &TemplateItem) -> Result<()> {
    // Ensure parent directory of output exists
    crate::processors::ensure_output_dir(&item.output_path)?;

    // Read template content
    let template_content = crate::errors::ctx(fs::read_to_string(&item.source_path), &format!("Failed to read template: {}", item.source_path.display()))?;

    // Create a new Tera instance for this template
    let mut tera = Tera::default();

    // Register template functions
    let ctx_ptr = CtxPtr(ctx as *const _);
    tera.register_function("load_python", LoadPythonFunction { ctx: ctx_ptr });
    tera.register_function("load_lua", LoadLuaFunction);
    tera.register_function("version_str", VersionStrFunction { ctx: ctx_ptr });
    tera.register_function("copyright_years", CopyrightYearsFunction { ctx: ctx_ptr });
    tera.register_function("git_count_files", GitCountFilesFunction { ctx: ctx_ptr });
    tera.register_function("workflow_names", WorkflowNamesFunction);
    tera.register_function("shell_output", ShellOutputFunction { ctx: ctx_ptr });

    // Add the template
    tera.add_raw_template("template", &template_content)
        .context("Failed to parse template")?;

    // Register all .tera files in the project so {% include %} can resolve them
    for entry in glob::glob("**/*.tera")
        .context("Invalid glob pattern: **/*.tera")?
    {
        let path = entry?;
        // Skip directories and the main template we already registered
        if !path.is_file() || path == item.source_path {
            continue;
        }
        let name = path.to_string_lossy().to_string();
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read template: {}", path.display()))?;
        tera.add_raw_template(&name, &content)
            .with_context(|| format!("Failed to parse template: {}", path.display()))?;
    }

    // Configure strict mode (fail on undefined variables)
    tera.set_escape_fn(|s| s.to_string()); // No HTML escaping by default

    // Create an empty context (load_python will be called from within the template)
    let context = TeraContext::new();

    // Render the template
    let rendered = tera
        .render("template", &context)
        .with_context(|| format!("Failed to render template: {}", item.source_path.display()))?;

    // Write to output file
    crate::errors::ctx(fs::write(&item.output_path, rendered), &format!("Failed to write output: {}", item.output_path.display()))?;

    Ok(())
}

pub struct TeraProcessor {
    config: TeraConfig,
}

impl TeraProcessor {
    pub fn new(config: TeraConfig) -> Self {
        Self {
            config,
        }
    }
}

impl Processor for TeraProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }


    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        super::find_templates(&self.config.standard, file_index).is_ok_and(|t| !t.is_empty())
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let items = super::find_templates(&self.config.standard, file_index)?;
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                instance_name,
                Some(output_config_hash(&self.config, <crate::config::TeraConfig as crate::config::KnownFields>::checksum_fields())),
            )?;
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        render_template(ctx, &item)
    }
}

/// Custom Tera function to load Python configuration files
struct LoadPythonFunction { ctx: CtxPtr }

impl Function for LoadPythonFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        // Get the path argument
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("load_python requires a 'path' argument"))?;

        // Execute Python and load the config
        let result = load_python_config(self.ctx.get(), Path::new(path))
            .map_err(|e| tera::Error::msg(format!("Failed to load Python config: {}", e)))?;

        to_value(result).map_err(|e| tera::Error::msg(format!("Failed to convert Python config to template value: {e}")))
    }
}

/// Custom Tera function to load Lua configuration files
struct LoadLuaFunction;

impl Function for LoadLuaFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("load_lua requires a 'path' argument"))?;

        let result = load_lua_config(Path::new(path))
            .map_err(|e| tera::Error::msg(format!("Failed to load Lua config: {e}")))?;

        to_value(result).map_err(|e| tera::Error::msg(format!("Failed to convert Lua config to template value: {e}")))
    }
}

/// Load a Python file containing a `tup` variable and return a dot-joined version string.
/// e.g. `tup = (0, 0, 1)` → `"0.0.1"`
struct VersionStrFunction { ctx: CtxPtr }

impl Function for VersionStrFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("config/version.py");

        let config = if path.ends_with(".lua") {
            load_lua_config(Path::new(path))
        } else {
            load_python_config(self.ctx.get(), Path::new(path))
        }
        .map_err(|e| tera::Error::msg(format!("version_str: failed to load {path}: {e}")))?;

        let tup = config
            .get("tup")
            .and_then(|v| v.as_array())
            .ok_or_else(|| tera::Error::msg(format!("version_str: no 'tup' array in {path}")))?;

        let version: Vec<String> = tup
            .iter()
            .map(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(String::from))
                    .unwrap_or_default()
            })
            .collect();

        to_value(version.join("."))
            .map_err(|e| tera::Error::msg(format!("version_str: {e}")))
    }
}

/// Return a comma-separated range of years from the first git commit year to the current year.
/// e.g. `"2013, 2014, 2015, ..., 2026"`
struct CopyrightYearsFunction { ctx: CtxPtr }

impl Function for CopyrightYearsFunction {
    fn call(&self, _args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let mut cmd = Command::new("git");
        cmd.args(["log", "--reverse", "--format=%ad", "--date=format:%Y"]);
        let output = run_command_capture(self.ctx.get(), &mut cmd)
            .map_err(|e| tera::Error::msg(format!("copyright_years: {e}")))?;

        if !output.status.success() {
            return Err(tera::Error::msg("copyright_years: git log failed"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_year: i32 = stdout
            .lines()
            .next()
            .and_then(|line| line.trim().parse().ok())
            .ok_or_else(|| tera::Error::msg("copyright_years: no commits found"))?;

        let current_year = chrono::Local::now().year();
        let years: Vec<String> = (first_year..=current_year).map(|y| y.to_string()).collect();

        to_value(years.join(", "))
            .map_err(|e| tera::Error::msg(format!("copyright_years: {e}")))
    }
}

/// Run `git ls-files -- "{pattern}"` and return the count of matching files.
struct GitCountFilesFunction { ctx: CtxPtr }

impl Function for GitCountFilesFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("git_count_files requires a 'pattern' argument"))?;

        let mut cmd = Command::new("git");
        cmd.args(["ls-files", "--", pattern]);
        let output = run_command_capture(self.ctx.get(), &mut cmd)
            .map_err(|e| tera::Error::msg(format!("git_count_files: {e}")))?;

        if !output.status.success() {
            return Err(tera::Error::msg("git_count_files: git ls-files failed"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let count = stdout.lines().filter(|l| !l.is_empty()).count();

        to_value(count)
            .map_err(|e| tera::Error::msg(format!("git_count_files: {e}")))
    }
}

/// Glob `.github/workflows/*.yml`, parse each YAML file's `name` field,
/// return array of objects `[{file: "build.yml", name: "build"}, ...]`.
struct WorkflowNamesFunction;

impl Function for WorkflowNamesFunction {
    fn call(&self, _args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let pattern = ".github/workflows/*.yml";
        let mut results = Vec::new();

        for entry in glob::glob(pattern)
            .map_err(|e| tera::Error::msg(format!("workflow_names: invalid glob: {e}")))?
        {
            let path = entry
                .map_err(|e| tera::Error::msg(format!("workflow_names: glob error: {e}")))?;

            let content = fs::read_to_string(&path)
                .map_err(|e| tera::Error::msg(format!("workflow_names: read {}: {e}", path.display())))?;

            let yaml: serde_yml::Value = serde_yml::from_str(&content)
                .map_err(|e| tera::Error::msg(format!("workflow_names: parse {}: {e}", path.display())))?;

            let name = yaml
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let file = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let mut entry_map = Map::new();
            entry_map.insert("file".to_string(), Value::String(file));
            entry_map.insert("name".to_string(), Value::String(name));
            results.push(Value::Object(entry_map));
        }

        to_value(results)
            .map_err(|e| tera::Error::msg(format!("workflow_names: {e}")))
    }
}

/// Run a shell command and return its trimmed stdout.
struct ShellOutputFunction { ctx: CtxPtr }

impl Function for ShellOutputFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("shell_output requires a 'command' argument"))?;

        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        let output = run_command_capture(self.ctx.get(), &mut cmd)
            .map_err(|e| tera::Error::msg(format!("shell_output: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(tera::Error::msg(format!("shell_output: command failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        to_value(stdout)
            .map_err(|e| tera::Error::msg(format!("shell_output: {e}")))
    }
}

/// Load configuration from a Python file
fn load_python_config(ctx: &crate::build_context::BuildContext, python_file: &Path) -> Result<Map<String, Value>> {
    // Resolve the path relative to current working directory
    let absolute_path = if python_file.is_absolute() {
        python_file.to_path_buf()
    } else {
        std::env::current_dir()?.join(python_file)
    };

    if !absolute_path.exists() {
        anyhow::bail!("Python config file not found: {}", absolute_path.display());
    }

    // Create a Python script that will execute the config file and output variables as JSON.
    // Escape backslashes and single quotes for safe embedding in Python string literals.
    let config_dir = absolute_path.parent().unwrap_or(Path::new(".")).display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let config_path = absolute_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let python_script = format!(
        r#"
import sys
import json
import os

# Set the working directory to the config file's directory
config_dir = '{}'
if config_dir:
    sys.path.insert(0, config_dir)

# Create a namespace for execution
namespace = {{}}

# Execute the config file
with open('{}', 'r') as f:
    exec(f.read(), namespace)

# Filter out built-in variables and convert to JSON-serializable format
result = {{}}
for key, value in namespace.items():
    if not key.startswith('__'):
        try:
            # Try to serialize the value
            json.dumps(value)
            result[key] = value
        except:
            # If not serializable, convert to string
            result[key] = str(value)

print(json.dumps(result))
"#,
        config_dir,
        config_path
    );

    // Execute Python and capture output
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(&python_script);
    let output = run_command_capture(ctx, &mut cmd)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Python config execution failed: {}", stderr);
    }

    // Parse the JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let variables: Map<String, Value> =
        crate::errors::ctx(serde_json::from_str(&stdout), "Failed to parse Python config output")?;

    Ok(variables)
}

/// Names of Lua built-in globals to skip when extracting user-defined variables.
const LUA_BUILTIN_GLOBALS: &[&str] = &[
    "string", "table", "math", "io", "os", "debug", "coroutine", "utf8", "package",
    "assert", "collectgarbage", "dofile", "error", "getmetatable", "ipairs", "load",
    "loadfile", "next", "pairs", "pcall", "print", "rawequal", "rawget", "rawlen",
    "rawset", "require", "select", "setmetatable", "tonumber", "tostring", "type",
    "warn", "xpcall",
];

/// Load configuration from a Lua file
fn load_lua_config(lua_file: &Path) -> Result<Map<String, Value>> {
    let absolute_path = if lua_file.is_absolute() {
        lua_file.to_path_buf()
    } else {
        std::env::current_dir()?.join(lua_file)
    };

    if !absolute_path.exists() {
        anyhow::bail!("Lua config file not found: {}", absolute_path.display());
    }

    let lua = Lua::new();

    // Set up package.path so require() works relative to the file's directory
    if let Some(dir) = absolute_path.parent() {
        let package: LuaTable = lua.globals().get("package")
            .map_err(|e| anyhow::anyhow!("Failed to get Lua package table: {e}"))?;
        let new_path = format!("{}/?.lua;{}/?.lua", dir.display(), dir.display());
        package.set("path", new_path)
            .map_err(|e| anyhow::anyhow!("Failed to set Lua package.path: {e}"))?;
    }

    // Load and execute the Lua file
    let script = fs::read_to_string(&absolute_path)
        .with_context(|| format!("Failed to read Lua config: {}", absolute_path.display()))?;
    lua.load(&script)
        .set_name(absolute_path.to_string_lossy())
        .exec()
        .map_err(|e| anyhow::anyhow!("Failed to execute Lua config '{}': {e}", absolute_path.display()))?;

    // Extract user-defined globals
    let globals = lua.globals();
    let mut result = Map::new();

    for pair in globals.pairs::<String, LuaValue>() {
        let (key, value) = pair
            .map_err(|e| anyhow::anyhow!("Failed to iterate Lua globals: {e}"))?;

        // Skip built-in globals and names starting with _
        if key.starts_with('_') || LUA_BUILTIN_GLOBALS.contains(&key.as_str()) {
            continue;
        }

        // Skip functions and non-serializable types
        match &value {
            LuaValue::Function(_) | LuaValue::Thread(_) | LuaValue::UserData(_)
            | LuaValue::LightUserData(_) => continue,
            _ => {}
        }

        // Convert to serde_json::Value using mlua's serde support
        let json_value: Value = lua.from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to convert Lua global '{key}' to JSON: {e}"))?;
        result.insert(key, json_value);
    }

    Ok(result)
}

use crate::registries as registry;

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    registry::deserialize_and_create(toml, |cfg| Box::new(TeraProcessor::new(cfg)))
}

inventory::submit! {
    registry::ProcessorPlugin {
        version: 1,
        name: "tera",
        processor_type: crate::processors::ProcessorType::Generator,
        create: plugin_create,
        defconfig_json: registry::default_config_json::<crate::config::TeraConfig>,
        known_fields: registry::typed_known_fields::<crate::config::TeraConfig>,
        checksum_fields: registry::typed_checksum_fields::<crate::config::TeraConfig>,
        must_fields: registry::typed_must_fields::<crate::config::TeraConfig>,
        field_descriptions: registry::typed_field_descriptions::<crate::config::TeraConfig>,
        keywords: &["template", "generator", "jinja", "html", "rust"],
        description: "Render Tera templates into output files",
        is_native: true,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: None,
    }
}
