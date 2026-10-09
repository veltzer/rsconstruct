//! A port of luacheck 1.2.0: Lua linting in-process.
//!
//! The modules mirror luacheck's: a lexer and parser producing luacheck's
//! AST, the check stages (linearization into per-function item graphs,
//! local resolution, and the detectors), inline options, the option and
//! standard machinery, `.luacheckrc` loading (run with the embedded Lua),
//! and the filter that applies options and resolves implicitly defined
//! globals across all files checked together. The `iluacheck` processor
//! (`src/processor/checker/iluacheck.rs`) is the command-line front end.

mod ast;
mod check;
mod config;
mod decoder;
mod filter;
mod format;
mod globals;
mod inline_options;
mod lexer;
mod linearize;
mod options;
mod parser;
mod pattern;
mod resolve;
mod stages;
mod standards;
mod unused;
mod value;

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use anyhow::Result;

use self::check::{CheckState, syntax_error_warning};
use self::decoder::Chars;
use self::filter::{CheckResult, FileInput, FileReport};
use self::value::LVal;

/// Data generated from luacheck's sources (`scripts/gen-iluacheck-tables.lua`).
pub struct Tables {
    pub standards: Vec<(String, serde_json::Value)>,
    pub printability_boundaries: Vec<u32>,
}

pub fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let json: serde_json::Value = serde_json::from_str(include_str!("tables.json"))
            .expect("luacheck tables.json is valid JSON");
        let standards = json["standards"]
            .as_array()
            .expect("standards array")
            .iter()
            .map(|pair| {
                (
                    pair[0].as_str().expect("std name").to_string(),
                    pair[1].clone(),
                )
            })
            .collect();
        let printability_boundaries = json["printability_boundaries"]
            .as_array()
            .expect("boundaries array")
            .iter()
            .map(|n| n.as_u64().expect("boundary") as u32)
            .collect();
        Tables {
            standards,
            printability_boundaries,
        }
    })
}

fn empty_state(
    source: Chars,
    line_offsets: Vec<Option<usize>>,
    line_lengths: Vec<Option<usize>>,
) -> CheckState {
    CheckState {
        source,
        line_offsets,
        line_lengths,
        warnings: Vec::new(),
        ast: ast::Ast::default(),
        root: 0,
        comments: Vec::new(),
        code_lines: HashMap::new(),
        line_endings: HashMap::new(),
        useless_semicolons: Vec::new(),
        vars: Vec::new(),
        values: Vec::new(),
        secondaries: Vec::new(),
        items: Vec::new(),
        lines: Vec::new(),
        top_line: 0,
        all_lines: Vec::new(),
        inline_options: Vec::new(),
    }
}

fn syntax_error_result(st: &CheckState, err: &parser::SyntaxError) -> CheckResult {
    CheckResult {
        warnings: vec![syntax_error_warning(st, err)],
        inline_options: Vec::new(),
        line_lengths: Vec::new(),
        line_endings: HashMap::new(),
    }
}

/// `check(source)`: one source through every stage.
pub fn check_source(bytes: Vec<u8>) -> CheckResult {
    let chars = Chars::decode(bytes);
    let (parsed, tables) = parser::parse(&chars);
    let mut st = empty_state(chars, tables.line_offsets, tables.line_lengths);
    let output = match parsed {
        Ok(output) => output,
        Err(err) => return syntax_error_result(&st, &err),
    };
    st.ast = output.ast;
    st.root = output.root;
    st.comments = output.comments;
    st.code_lines = output.code_lines;
    st.line_endings = output.line_endings;
    st.useless_semicolons = output.hanging_semicolons;

    stages::unwrap_parens(&mut st);
    if let Err(err) = linearize::run(&mut st) {
        return syntax_error_result(&st, &err);
    }
    inline_options::run(&mut st);
    stages::name_functions(&mut st);
    resolve::run(&mut st);
    stages::detect_bad_whitespace(&mut st);
    stages::detect_compound_operators(&mut st);
    stages::detect_cyclomatic_complexity(&mut st);
    stages::detect_empty_blocks(&mut st);
    stages::detect_empty_statements(&mut st);
    globals::run(&mut st);
    stages::detect_reversed_fornum_loops(&mut st);
    stages::detect_unbalanced_assignments(&mut st);
    stages::detect_uninit_accesses(&mut st);
    stages::detect_unreachable_code(&mut st);
    stages::detect_unused_fields(&mut st);
    unused::run(&mut st);

    let mut warnings = std::mem::take(&mut st.warnings);
    filter::sort_warnings(&mut warnings);
    CheckResult {
        warnings,
        inline_options: std::mem::take(&mut st.inline_options),
        line_lengths: std::mem::take(&mut st.line_lengths),
        line_endings: std::mem::take(&mut st.line_endings),
    }
}

/// A limit given on the command line: a number, or `--no-max-...`.
#[derive(Clone, Copy, Debug)]
pub enum Limit {
    Value(f64),
    Unlimited,
}

/// The options luacheck's command line can set.
#[derive(Clone, Debug, Default)]
pub struct CliOptions {
    /// The configuration file to look up (`--config`); `None` for `--no-config`.
    pub config: Option<String>,
    pub std: Option<String>,
    pub globals: Option<Vec<String>>,
    pub read_globals: Option<Vec<String>>,
    pub new_globals: Option<Vec<String>>,
    pub new_read_globals: Option<Vec<String>>,
    pub not_globals: Option<Vec<String>>,
    pub ignore: Option<Vec<String>>,
    pub enable: Option<Vec<String>>,
    pub only: Option<Vec<String>>,
    pub operators: Option<Vec<String>>,
    pub allow_defined: bool,
    pub allow_defined_top: bool,
    pub module: bool,
    /// `-g`, `-u`, `-r`, `-a`, `-s`, `--no-self`: options set to false.
    pub disabled: Vec<&'static str>,
    pub max_line_length: Option<Limit>,
    pub max_code_line_length: Option<Limit>,
    pub max_string_line_length: Option<Limit>,
    pub max_comment_line_length: Option<Limit>,
    pub max_cyclomatic_complexity: Option<Limit>,
}

impl CliOptions {
    fn entries(&self) -> BTreeMap<&'static str, LVal> {
        let mut entries = BTreeMap::new();
        let strings =
            |list: &[String]| LVal::table(list.iter().map(|s| LVal::str(s)).collect(), Vec::new());
        if let Some(std) = &self.std {
            entries.insert("std", LVal::str(std));
        }
        for (name, list) in [
            ("globals", &self.globals),
            ("read_globals", &self.read_globals),
            ("new_globals", &self.new_globals),
            ("new_read_globals", &self.new_read_globals),
            ("not_globals", &self.not_globals),
            ("ignore", &self.ignore),
            ("enable", &self.enable),
            ("only", &self.only),
            ("operators", &self.operators),
        ] {
            if let Some(list) = list {
                entries.insert(name, strings(list));
            }
        }
        for (name, set) in [
            ("allow_defined", self.allow_defined),
            ("allow_defined_top", self.allow_defined_top),
            ("module", self.module),
        ] {
            if set {
                entries.insert(name, LVal::Bool(true));
            }
        }
        for &name in &self.disabled {
            entries.insert(name, LVal::Bool(false));
        }
        for (name, limit) in [
            ("max_line_length", self.max_line_length),
            ("max_code_line_length", self.max_code_line_length),
            ("max_string_line_length", self.max_string_line_length),
            ("max_comment_line_length", self.max_comment_line_length),
            ("max_cyclomatic_complexity", self.max_cyclomatic_complexity),
        ] {
            match limit {
                Some(Limit::Value(n)) => {
                    entries.insert(name, LVal::Num(n));
                }
                Some(Limit::Unlimited) => {
                    entries.insert(name, LVal::Bool(false));
                }
                None => {}
            }
        }
        entries
    }
}

/// The result for one checked file.
pub enum FileResult {
    /// The file was checked: its findings as `luacheck --formatter plain
    /// --codes` prints them (empty when clean).
    Checked(Vec<String>),
    /// The file could not be checked (`I/O error`).
    Fatal(String),
}

/// Checks files together, as one `luacheck` invocation does. Files that
/// `exclude_files`/`include_files` filter out are absent from the result.
pub fn run(files: &[String], cli: &CliOptions) -> Result<Vec<(String, FileResult)>> {
    let current_dir = config::current_dir()?;
    let base = config::load_config(cli.config.as_deref(), &current_dir)?;
    let override_config = config::cli_config(cli.entries());
    let stack = config::stack_configs(vec![base, override_config])?;
    let (exclude, include) = stack.file_filters(&current_dir);
    let invalid = |e: pattern::PatternError, p: &str| anyhow::anyhow!("Invalid pattern '{p}': {e}");

    let mut names = Vec::new();
    for file in files {
        let path = match file.as_bytes() {
            [b'.', b'/', rest @ ..] if rest.first().is_some_and(|&c| c != b'/') => {
                file[2..].to_string()
            }
            _ => file.clone(),
        };
        let abs = config::normalize(&config::join(&current_dir, &path));
        let excluded =
            config::matches_any(&exclude, &abs).map_err(|e| invalid(e, "exclude_files"))?;
        let included = include.is_empty()
            || config::matches_any(&include, &abs).map_err(|e| invalid(e, "include_files"))?;
        if !excluded && included {
            names.push(path);
        }
    }

    let mut inputs = Vec::new();
    for name in &names {
        match std::fs::read(name) {
            Ok(bytes) => {
                let bytes = bytes
                    .strip_prefix(b"\xEF\xBB\xBF")
                    .map(<[u8]>::to_vec)
                    .unwrap_or(bytes);
                let result = check_source(bytes);
                let stack = stack
                    .get_options(name, &current_dir)
                    .map_err(|e| invalid(e, "files"))?;
                inputs.push(FileInput::Checked { result, stack });
            }
            Err(e) => inputs.push(FileInput::Fatal {
                kind: "I/O",
                msg: format!("couldn't read: {e}"),
            }),
        }
    }
    let reports = filter::filter(inputs, &stack.stds).map_err(|e| {
        anyhow::anyhow!("Invalid pattern '{}'", String::from_utf8_lossy(&e.pattern))
    })?;
    let mut out = Vec::new();
    for (name, report) in names.into_iter().zip(reports) {
        let result = match report {
            FileReport::Fatal { kind, msg } => {
                FileResult::Fatal(format!("{name}: {kind} error ({msg})"))
            }
            FileReport::Warnings(warnings) => FileResult::Checked(
                warnings
                    .iter()
                    .map(|w| String::from_utf8_lossy(&format::plain_line(&name, w)).into_owned())
                    .collect(),
            ),
        };
        out.push((name, result));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
