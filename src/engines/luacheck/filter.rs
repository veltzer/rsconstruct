//! luacheck's `filter` module: applies options (config, per-path overrides,
//! inline comments) to check results, resolves implicitly defined globals
//! across all files checked together, and sorts what is left.

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use super::check::{IndexKey, Warning};
use super::decoder;
use super::inline_options::{InlineOption, InlineOpts};
use super::options::{self, InvalidPattern, Normalized, OptionStack, Stds};
use super::parser::LineEnding;
use super::standards::Def;
use super::value::LTable;

/// A file's check result.
pub struct CheckResult {
    pub warnings: Vec<Warning>,
    pub inline_options: Vec<InlineOption>,
    pub line_lengths: Vec<Option<usize>>,
    pub line_endings: HashMap<usize, LineEnding>,
}

struct FileState {
    result: CheckResult,
    filtered: Vec<Warning>,
    normalized_options: HashMap<usize, Rc<Normalized>>,
}

fn sort_by_location(warnings: &mut [Warning]) {
    warnings.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.code.cmp(&b.code))
    });
}

pub fn sort_warnings(warnings: &mut [Warning]) {
    sort_by_location(warnings);
}

fn field_string(warning: &Warning) -> Vec<u8> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for index in warning.indexing.iter().flatten() {
        parts.push(match index {
            IndexKey::Str(s) => decoder::printable(s),
            _ => b"?".to_vec(),
        });
    }
    parts.join(&b'.')
}

#[derive(PartialEq, Eq)]
enum FieldStatus {
    ReadOnly,
    Global,
    Undefined,
}

fn field_status(opts: &Normalized, warning: &Warning, depth: Option<usize>) -> FieldStatus {
    let mut def: &Def = &opts.std;
    let mut defined = true;
    let mut read_only = true;
    let indexing = warning.indexing.as_deref().unwrap_or(&[]);
    let depth = depth.unwrap_or(indexing.len() + 1);
    for i in 1..=depth {
        let key = if i == 1 {
            IndexKey::Str(warning.name.clone().unwrap_or_default())
        } else {
            indexing[i - 2].clone()
        };
        match key {
            IndexKey::Unknown => {
                if def.fields.as_ref().is_some_and(|f| !f.is_empty())
                    || def.other_fields.unwrap_or(false)
                {
                    read_only = def.deep_read_only;
                } else {
                    defined = false;
                }
                break;
            }
            IndexKey::NotString => {
                if !def.other_fields.unwrap_or(false) {
                    defined = false;
                }
                break;
            }
            IndexKey::Str(name) => {
                if let Some(next) = def.fields.as_ref().and_then(|f| f.get(&name)) {
                    def = next;
                    if let Some(ro) = def.read_only {
                        read_only = ro;
                    }
                } else {
                    if !def.other_fields.unwrap_or(false) {
                        defined = false;
                    }
                    break;
                }
            }
        }
    }
    if !defined {
        FieldStatus::Undefined
    } else if read_only {
        FieldStatus::ReadOnly
    } else {
        FieldStatus::Global
    }
}

fn passes_filter(opts: &Normalized, warning: &mut Warning) -> Result<bool, InvalidPattern> {
    let code = warning.code.clone();
    if code == "561" {
        let Some(max_complexity) = opts.max_cyclomatic_complexity else {
            return Ok(false);
        };
        if warning.complexity.unwrap_or(0) as f64 <= max_complexity {
            return Ok(false);
        }
        warning.max_complexity = Some(max_complexity);
    } else if code == "033" {
        let operator = warning.operator.clone().unwrap_or_default();
        let allowed = opts
            .operators
            .as_ref()
            .is_some_and(|ops| ops.contains(&operator));
        return Ok(!allowed);
    } else if matches!(code.as_bytes()[0], b'2' | b'3' | b'4')
        && warning.name.as_deref() == Some(b"_")
        && !warning.useless
    {
        return Ok(false);
    } else if code.starts_with("11") || code.starts_with("14") {
        if warning.indirect
            && field_status(opts, warning, warning.previous_indexing_len) == FieldStatus::Undefined
        {
            return Ok(false);
        }
        if !warning.module && field_status(opts, warning, None) != FieldStatus::Undefined {
            return Ok(false);
        }
    }
    let b = code.as_bytes();
    if b[0] == b'1' && matches!(b[1], b'2' | b'4') && matches!(b[2], b'2' | b'3') {
        warning.field = Some(field_string(warning));
    }
    if warning.secondary && !opts.unused_secondaries {
        return Ok(false);
    }
    if warning.is_self && !opts.is_self {
        return Ok(false);
    }
    options::passes_rules_filter(&opts.rules, &code, warning.name.as_deref())
}

/// Caches normalized options by the identities of an option stack's tables.
#[derive(Default)]
struct Normalizer {
    cache: HashMap<Vec<usize>, Rc<Normalized>>,
}

impl Normalizer {
    fn normalize(&mut self, stds: &Stds, stack: &OptionStack) -> Rc<Normalized> {
        let key: Vec<usize> = stack.iter().map(|t| t.id).collect();
        self.cache
            .entry(key)
            .or_insert_with(|| Rc::new(options::normalize(stack, stds)))
            .clone()
    }
}

fn filter_not_global_related_in_file(
    file: &mut FileState,
    normalizer: &mut Normalizer,
    stds: &Stds,
    mut stack: OptionStack,
    empty_options: &Rc<LTable>,
) -> Result<(), InvalidPattern> {
    let mut next_warning = 0usize;
    let mut next_inline = 0usize;
    let mut line = 1usize;
    while let Some(Some(line_length)) = file.result.line_lengths.get(line).copied() {
        // update_option_stack_for_new_line
        if let Some(inline) = file.result.inline_options.get(next_inline).cloned()
            && inline.line <= line
        {
            next_inline += 1;
            for _ in 0..inline.pop_count.unwrap_or(0) {
                stack.pop();
            }
            if let Some(InlineOpts::Table(opts)) = &inline.options {
                match options::validate(
                    &options::all_options(),
                    &super::value::LVal::Table(opts.clone()),
                    stds,
                ) {
                    Ok(()) => stack.push(opts.clone()),
                    Err(msg) => {
                        let mut warning = Warning::new("021");
                        warning.line = inline.line;
                        warning.column = inline.column.unwrap_or(0);
                        warning.end_column = inline.end_column.unwrap_or(0);
                        warning.msg = Some(msg.into_bytes());
                        file.filtered.push(warning);
                        stack.push(empty_options.clone());
                    }
                }
            }
        }
        let line_opts = normalizer.normalize(stds, &stack);

        // check_line_length
        let line_type = file.result.line_endings.get(&line).copied();
        let max_length = match line_type {
            None => line_opts.max_code_line_length,
            Some(LineEnding::String) => line_opts.max_string_line_length,
            Some(LineEnding::Comment) => line_opts.max_comment_line_length,
        };
        if let Some(max_length) = max_length
            && line_length as f64 > max_length
            && options::passes_rules_filter(&line_opts.rules, "631", None)?
        {
            let mut warning = Warning::new("631");
            warning.line = line;
            warning.column = (max_length + 1.0) as usize;
            warning.end_column = line_length;
            warning.max_length = Some(max_length);
            file.filtered.push(warning);
        }

        // filter_warnings_on_new_line
        while let Some(warning) = file.result.warnings.get(next_warning) {
            if warning.line > line {
                break;
            }
            if warning.code.starts_with('1') {
                file.normalized_options.insert(line, line_opts.clone());
            } else {
                let mut warning = warning.clone();
                if passes_filter(&line_opts, &mut warning)? {
                    file.filtered.push(warning);
                }
            }
            next_warning += 1;
        }
        line += 1;
    }
    Ok(())
}

const fn is_definition(opts: &Normalized, warning: &Warning) -> bool {
    opts.allow_defined || (opts.allow_defined_top && warning.top)
}

/// The final report of one file: warnings, or an error that kept it from
/// being checked.
pub enum FileReport {
    Fatal { kind: &'static str, msg: String },
    Warnings(Vec<Warning>),
}

/// Input to [`filter`]: a check result (or a fatal error) and the option
/// stack that applies to the file.
pub enum FileInput {
    Fatal {
        kind: &'static str,
        msg: String,
    },
    Checked {
        result: CheckResult,
        stack: OptionStack,
    },
}

/// `filter.filter`.
pub fn filter(inputs: Vec<FileInput>, stds: &Stds) -> Result<Vec<FileReport>, InvalidPattern> {
    let mut normalizer = Normalizer::default();
    let empty_options = Rc::new(LTable::default());
    let mut files: Vec<Option<FileState>> = Vec::new();
    let mut fatals: Vec<Option<(&'static str, String)>> = Vec::new();

    for input in inputs {
        match input {
            FileInput::Fatal { kind, msg } => {
                files.push(None);
                fatals.push(Some((kind, msg)));
            }
            FileInput::Checked { result, stack } => {
                let mut file = FileState {
                    result,
                    filtered: Vec::new(),
                    normalized_options: HashMap::new(),
                };
                if file
                    .result
                    .warnings
                    .first()
                    .is_some_and(|w| w.code == "011")
                {
                    file.filtered.clone_from(&file.result.warnings);
                } else {
                    filter_not_global_related_in_file(
                        &mut file,
                        &mut normalizer,
                        stds,
                        stack,
                        &empty_options,
                    )?;
                }
                files.push(Some(file));
                fatals.push(None);
            }
        }
    }

    // get_implicit_globals
    let mut globally_defined: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut globally_used: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut locally_defined: Vec<BTreeSet<Vec<u8>>> = Vec::new();
    for file in &files {
        let mut defined = BTreeSet::new();
        if let Some(file) = file {
            for warning in &file.result.warnings {
                if !warning.code.starts_with("11") {
                    continue;
                }
                let name = warning.name.clone().unwrap_or_default();
                if warning.code == "111" {
                    let Some(opts) = file.normalized_options.get(&warning.line) else {
                        continue;
                    };
                    if is_definition(opts, warning) {
                        if opts.module {
                            defined.insert(name);
                        } else {
                            globally_defined.insert(name);
                        }
                    }
                } else {
                    globally_used.insert(name);
                }
            }
        }
        locally_defined.push(defined);
    }

    // filter_global_related
    for (index, file) in files.iter_mut().enumerate() {
        let Some(file) = file else { continue };
        let warnings = file.result.warnings.clone();
        for warning in warnings {
            if !warning.code.starts_with('1') {
                continue;
            }
            let Some(opts) = file.normalized_options.get(&warning.line).cloned() else {
                continue;
            };
            let mut warning = warning;
            let name = warning.name.clone().unwrap_or_default();
            // apply_implicit_definitions
            if warning.code.starts_with("11") {
                if warning.code == "111" {
                    if opts.module {
                        if locally_defined[index].contains(&name) {
                            continue;
                        }
                        warning.module = true;
                    } else if is_definition(&opts, &warning) {
                        if globally_used.contains(&name) {
                            continue;
                        }
                        warning.code = "131".into();
                        warning.top = false;
                    } else if globally_defined.contains(&name) {
                        continue;
                    }
                } else if globally_defined.contains(&name) || locally_defined[index].contains(&name)
                {
                    continue;
                }
            }
            if (warning.code == "111" || warning.code == "112")
                && !warning.module
                && field_status(&opts, &warning, None) == FieldStatus::ReadOnly
            {
                warning.code = format!("12{}", &warning.code[2..3]);
            } else if (warning.code == "112" || warning.code == "113")
                && field_status(&opts, &warning, Some(1)) != FieldStatus::Undefined
            {
                warning.code = format!("14{}", &warning.code[2..3]);
            }
            if (warning.code.contains("112") || warning.code.contains("113"))
                && field_status(&opts, &warning, Some(1)) != FieldStatus::Undefined
            {
                warning.code = format!("14{}", &warning.code[2..3]);
            }
            if passes_filter(&opts, &mut warning)? {
                file.filtered.push(warning);
            }
        }
    }

    let mut reports = Vec::new();
    for (file, fatal) in files.into_iter().zip(fatals) {
        match (file, fatal) {
            (Some(mut file), _) => {
                sort_by_location(&mut file.filtered);
                reports.push(FileReport::Warnings(file.filtered));
            }
            (None, Some((kind, msg))) => reports.push(FileReport::Fatal { kind, msg }),
            (None, None) => unreachable!("file without result"),
        }
    }
    Ok(reports)
}
