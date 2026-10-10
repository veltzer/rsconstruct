//! A port of `ShellCheck` 0.11.0: shell script analysis in-process.
//!
//! The modules mirror `ShellCheck`'s: the AST (`ast`) and its helpers
//! (`astlib`), a Parsec engine (`parsec`) with the parser on top of it
//! (`parser`, `parser_cond`, `parser_cmd`), and the analyzer (`analyzer`
//! and the check modules). This file is `ShellCheck`'s `Checker` plus the
//! parts of its command line that decide what a check sees: reading files,
//! following `source`, finding `.shellcheckrc`, and the gcc output format.
//! The `ishellcheck` processor (`src/processor/checker/ishellcheck.rs`) is
//! the front end.

pub mod analytics;
pub mod analytics2;
pub mod analytics3;
pub mod analyzer;
pub mod analyzerlib;
pub mod ast;
pub mod astlib;
pub mod cfg;
pub mod cfganalysis;
pub mod commands;
pub mod data;
pub mod hchar;
pub mod interface;
pub mod parsec;
pub mod parser;
pub mod parser_cmd;
pub mod parser_cond;
pub mod regex;
pub mod shellsupport;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use self::interface::{PositionedComment, Severity, Shell};
use self::parsec::SystemInterface;
use self::parser_cmd::{ParseSpec, parse_script};

/// `CheckSpec`.
#[derive(Clone, Debug)]
pub struct CheckSpec {
    pub filename: String,
    pub script: String,
    pub check_sourced: bool,
    pub ignore_rc: bool,
    pub excluded_warnings: Vec<i64>,
    pub included_warnings: Option<Vec<i64>>,
    pub shell_type_override: Option<Shell>,
    pub min_severity: Severity,
    pub extended_analysis: Option<bool>,
    pub optional_checks: Vec<String>,
}

impl CheckSpec {
    pub fn new(filename: &str, script: String) -> Self {
        Self {
            filename: filename.to_string(),
            script,
            check_sourced: false,
            ignore_rc: false,
            excluded_warnings: Vec::new(),
            included_warnings: None,
            shell_type_override: None,
            min_severity: Severity::StyleC,
            extended_analysis: None,
            optional_checks: Vec::new(),
        }
    }
}

/// `shellFromFilename`.
fn shell_from_filename(filename: &str) -> Option<Shell> {
    [
        (".ksh", Shell::Ksh),
        (".bash", Shell::Bash),
        (".bats", Shell::Bash),
        (".dash", Shell::Dash),
    ]
    .iter()
    .find(|(ext, _)| filename.ends_with(ext))
    .map(|(_, sh)| *sh)
}

/// `checkScript`: the comments for one script, sorted and deduplicated.
pub fn check_script(sys: &Rc<dyn SystemInterface>, spec: &CheckSpec) -> Vec<PositionedComment> {
    let result = parse_script(
        Rc::clone(sys),
        &ParseSpec {
            filename: spec.filename.clone(),
            script: spec.script.clone(),
            check_sourced: spec.check_sourced,
            ignore_rc: spec.ignore_rc,
            shell_type_override: spec.shell_type_override,
        },
    );
    let mut comments = result.comments;
    if let Some(root) = &result.root {
        let mut optional_checks = astlib::get_enable_directives(root);
        optional_checks.extend(spec.optional_checks.iter().cloned());
        let analysis = analyzer::AnalysisSpec {
            script: root,
            shell_type: spec.shell_type_override,
            fallback_shell: shell_from_filename(&spec.filename),
            check_sourced: spec.check_sourced,
            optional_checks,
            extended_analysis: spec.extended_analysis,
            token_positions: &result.token_positions,
        };
        for tc in analyzer::analyze_script(&analysis) {
            let Some((start, end)) = result.token_positions.get(&tc.id) else {
                panic!("Internal shellcheck error: id doesn't exist. Please report!");
            };
            comments.push(PositionedComment {
                start: start.clone(),
                end: end.clone(),
                comment: tc.comment,
                fix: tc.fix,
            });
        }
    }
    let should_include = |pc: &PositionedComment| {
        let code = pc.comment.code;
        pc.comment.severity <= spec.min_severity
            && match &spec.included_warnings {
                None => !spec.excluded_warnings.contains(&code),
                Some(included) => included.contains(&code),
            }
    };
    let mut kept: Vec<PositionedComment> = comments.into_iter().filter(should_include).collect();
    kept.sort_by(|a, b| {
        (
            &a.start.file,
            a.start.line,
            a.start.column,
            a.comment.severity,
            a.comment.code,
            &a.comment.message,
        )
            .cmp(&(
                &b.start.file,
                b.start.line,
                b.start.column,
                b.comment.severity,
                b.comment.code,
                &b.comment.message,
            ))
    });
    let mut out: Vec<PositionedComment> = Vec::new();
    for c in kept {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// `decodeString`: UTF-8, with every byte that does not start a valid
/// sequence taken as ISO-8859-1.
pub fn decode_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte < 0x80 {
            out.push(byte as char);
            i += 1;
            continue;
        }
        let (init, n) = match byte {
            0xF8..=0xFF => (0, usize::MAX),
            0xF0..=0xF7 => (u32::from(byte & 0x07), 3),
            0xE0..=0xEF => (u32::from(byte & 0x0F), 2),
            0xC0..=0xDF => (u32::from(byte & 0x1F), 1),
            _ => (0, usize::MAX),
        };
        let mut decoded = None;
        if n != usize::MAX && i + n < bytes.len() + 1 {
            let mut code = init;
            let mut ok = true;
            for k in 1..=n {
                match bytes.get(i + k) {
                    Some(&c) if (0x80..=0xBF).contains(&c) => {
                        code = (code << 6) | u32::from(c & 0x3F);
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && code <= 0x10_FFFF {
                decoded = Some((char::from_u32(code).unwrap_or('\u{FFFD}'), n + 1));
            }
        }
        if let Some((c, len)) = decoded {
            out.push(c);
            i += len;
        } else {
            out.push(char::from(byte));
            i += 1;
        }
    }
    out
}

/// Command-line options that shape the system interface.
#[derive(Clone, Debug, Default)]
pub struct IoOptions {
    /// `-x` / `--external-sources`.
    pub external_sources: bool,
    /// `-P` / `--source-path`.
    pub source_paths: Vec<String>,
    /// `--rcfile`.
    pub rcfile: Option<String>,
}

/// shellcheck's `ioInterface`: files from disk.
pub struct IoInterface {
    options: IoOptions,
    inputs: Vec<PathBuf>,
    cache: RefCell<HashMap<String, String>>,
    config_cache: RefCell<(String, Option<(String, String)>)>,
}

fn normalize(x: &str) -> PathBuf {
    std::fs::canonicalize(x).unwrap_or_else(|_| PathBuf::from(x))
}

fn read_input_file(file: &str) -> Result<String, String> {
    std::fs::read(file)
        .map(|b| decode_string(&b))
        .map_err(|e| io_error_text(file, &e))
}

/// The text of a Haskell `IOException` for a failed open.
fn io_error_text(file: &str, e: &std::io::Error) -> String {
    let what = match e.kind() {
        std::io::ErrorKind::NotFound => "does not exist (No such file or directory)".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied (Permission denied)".to_string(),
        _ => {
            if e.raw_os_error() == Some(21) {
                "inappropriate type (is a directory)".to_string()
            } else {
                e.to_string()
            }
        }
    };
    format!("{file}: openBinaryFile: {what}")
}

impl IoInterface {
    pub fn new(options: IoOptions, files: &[String]) -> Self {
        Self {
            options,
            inputs: files.iter().map(|f| normalize(f)).collect(),
            cache: RefCell::new(HashMap::new()),
            config_cache: RefCell::new((String::new(), None)),
        }
    }

    fn allowable(&self, rc_suggests_external: Option<bool>, x: &str) -> bool {
        if rc_suggests_external.unwrap_or(self.options.external_sources) {
            return true;
        }
        self.inputs.contains(&normalize(x))
    }

    fn read_config(file: &str) -> Option<(String, String)> {
        if !Path::new(file).is_file() {
            return None;
        }
        Some((file.to_string(), read_input_file(file).unwrap_or_default()))
    }

    fn config_paths(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut d = dir.to_path_buf();
        loop {
            out.push(d.join(".shellcheckrc").to_string_lossy().into_owned());
            out.push(d.join("shellcheckrc").to_string_lossy().into_owned());
            match d.parent() {
                Some(p) => d = p.to_path_buf(),
                None => break,
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            out.push(
                Path::new(&home)
                    .join(".shellcheckrc")
                    .to_string_lossy()
                    .into_owned(),
            );
            let xdg = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty() && Path::new(v).is_absolute())
                .map_or_else(|| Path::new(&home).join(".config"), PathBuf::from);
            out.push(xdg.join("shellcheckrc").to_string_lossy().into_owned());
        }
        out
    }
}

impl SystemInterface for IoInterface {
    fn read_file(&self, rc_suggests_external: Option<bool>, file: &str) -> Result<String, String> {
        if let Some(x) = self.cache.borrow().get(file) {
            return Ok(x.clone());
        }
        if self.allowable(rc_suggests_external, file) {
            // Regular files are re-readable, so shellcheck does not cache them.
            read_input_file(file)
        } else if rc_suggests_external == Some(false) {
            Err(format!(
                "{file} was not specified as input, and external files were disabled via directive."
            ))
        } else {
            Err(format!(
                "{file} was not specified as input (see shellcheck -x)."
            ))
        }
    }

    fn find_source(
        &self,
        current_script: &str,
        rc_suggests_external: Option<bool>,
        paths: &[String],
        original: &str,
    ) -> String {
        // dropFileName
        let scriptdir = match current_script.rfind('/') {
            Some(i) => &current_script[..=i],
            None => "",
        };
        let adjust_path = |s: &str| -> String {
            let dirs = split_directories(s);
            if dirs.first().map(String::as_str) == Some("SCRIPTDIR") {
                let mut parts = vec![scriptdir.to_string()];
                parts.extend(dirs[1..].iter().cloned());
                join_path(&parts)
            } else {
                s.to_string()
            }
        };
        // On POSIX, splitDrive "/a/b" is ("/", "a/b"): an absolute name is
        // looked up relative to each candidate directory first.
        let filename = if original.starts_with('/') {
            original.trim_start_matches('/')
        } else {
            original
        };
        let mut candidates = vec![adjust_path(filename)];
        for p in self.options.source_paths.iter().chain(paths.iter()) {
            candidates.push(combine_path(&adjust_path(p), filename));
        }
        for c in candidates {
            if self.allowable(rc_suggests_external, &c) && Path::new(&c).is_file() {
                return c;
            }
        }
        original.to_string()
    }

    fn get_config(&self, filename: &str) -> Option<(String, String)> {
        if let Some(file) = &self.options.rcfile {
            let mut cache = self.config_cache.borrow_mut();
            if cache.0 == "/" {
                return cache.1.clone();
            }
            let result = Self::read_config(file);
            if result.is_none() {
                eprintln!("Warning: unable to read --rcfile {file}");
            }
            *cache = ("/".to_string(), result.clone());
            return result;
        }
        let path = normalize(filename);
        let dir = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let dir_s = dir.to_string_lossy().into_owned();
        {
            let cache = self.config_cache.borrow();
            if cache.0 == dir_s {
                return cache.1.clone();
            }
        }
        let result = Self::config_paths(&dir)
            .into_iter()
            .find_map(|f| Self::read_config(&f));
        *self.config_cache.borrow_mut() = (dir_s, result.clone());
        result
    }
}

/// filepath's `splitDirectories`.
fn split_directories(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    if s.starts_with('/') {
        out.push("/".to_string());
    }
    out.extend(s.split('/').filter(|p| !p.is_empty()).map(str::to_string));
    out
}

/// filepath's `</>`.
fn combine_path(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_string()
    } else if b.is_empty() {
        a.to_string()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// filepath's `joinPath`.
fn join_path(parts: &[String]) -> String {
    parts
        .iter()
        .rev()
        .fold(String::new(), |acc, p| combine_path(p, &acc))
}

/// `removeTabStops`: a column with tabs at 8 as the column in characters.
fn real_column(line: &str, target: i64) -> i64 {
    let mut r: i64 = 0;
    let mut v: i64 = 0;
    for c in line.chars() {
        if target <= v {
            return r;
        }
        r += 1;
        v = if c == '\t' { v + 8 - (v % 8) } else { v + 1 };
    }
    if target <= v { r } else { r + (target - v) }
}

/// The gcc format: `file:line:col: severity: message [SCcode]`, columns
/// counted in characters as `makeNonVirtual` does.
pub fn format_gcc(comments: &[PositionedComment], read: &dyn Fn(&str) -> String) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < comments.len() {
        let file = &comments[i].start.file;
        let mut j = i;
        while j < comments.len() && &comments[j].start.file == file {
            j += 1;
        }
        let contents = read(file);
        let lines: Vec<&str> = parser::haskell_lines(&contents);
        for c in &comments[i..j] {
            let line_no = c.start.line;
            let col = if line_no > 0 && (line_no as usize) <= lines.len() {
                real_column(lines[line_no as usize - 1], c.start.column)
            } else {
                c.start.column
            };
            let message: String = parser::haskell_lines(&c.comment.message).concat();
            out.push(format!(
                "{}:{}:{}: {}: {} [SC{}]",
                file,
                line_no,
                col,
                c.comment.severity.gcc_name(),
                message,
                c.comment.code
            ));
        }
        i = j;
    }
    out
}

/// Checks files as `shellcheck FILE...` does, returning gcc-format lines.
pub fn check_files(files: &[String], io: IoOptions, template: &CheckSpec) -> Vec<String> {
    let interface = Rc::new(IoInterface::new(io, files));
    let sys: Rc<dyn SystemInterface> = interface.clone();
    let mut out = Vec::new();
    for file in files {
        match interface.read_file(None, file) {
            Err(e) => out.push(format!("{file}: {e}")),
            Ok(contents) => {
                let mut spec = template.clone();
                spec.filename.clone_from(file);
                spec.script = contents;
                let comments = check_script(&sys, &spec);
                let read = |f: &str| interface.read_file(Some(true), f).unwrap_or_default();
                out.extend(format_gcc(&comments, &read));
            }
        }
    }
    out
}
