use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::tables;
use anyhow::{Context, Result};

use crate::file_index::FileIndex;
use crate::json_output::{self, SlocCocomoEstimate, SlocLanguageEntry, SlocOutput, SlocTotals};

struct CommentStyle {
    single: &'static [&'static str],
    block_start: Option<&'static str>,
    block_end: Option<&'static str>,
}

static COMMENT_C_STYLE: CommentStyle = CommentStyle {
    single: &["//"],
    block_start: Some("/*"),
    block_end: Some("*/"),
};

static COMMENT_HASH: CommentStyle = CommentStyle {
    single: &["#"],
    block_start: None,
    block_end: None,
};

static COMMENT_HTML: CommentStyle = CommentStyle {
    single: &[],
    block_start: Some("<!--"),
    block_end: Some("-->"),
};

static COMMENT_DASH: CommentStyle = CommentStyle {
    single: &["--"],
    block_start: None,
    block_end: None,
};

static COMMENT_PERCENT: CommentStyle = CommentStyle {
    single: &["%"],
    block_start: None,
    block_end: None,
};

static COMMENT_SEMICOLON: CommentStyle = CommentStyle {
    single: &[";"],
    block_start: None,
    block_end: None,
};

static COMMENT_NONE: CommentStyle = CommentStyle {
    single: &[],
    block_start: None,
    block_end: None,
};

struct LanguageInfo {
    name: &'static str,
    comment: &'static CommentStyle,
}

/// Every language sloc knows: the extensions (with their dot) or exact file
/// names that select it, its display name, and how it writes comments.
static LANGUAGES: &[(&[&str], &str, &CommentStyle)] = &[
    (&[".rs"], "Rust", &COMMENT_C_STYLE),
    (&[".c", ".h"], "C", &COMMENT_C_STYLE),
    (
        &[".cc", ".cpp", ".cxx", ".hh", ".hpp", ".hxx"],
        "C++",
        &COMMENT_C_STYLE,
    ),
    (&[".cs"], "C#", &COMMENT_C_STYLE),
    (&[".java"], "Java", &COMMENT_C_STYLE),
    (&[".js", ".mjs", ".cjs"], "JavaScript", &COMMENT_C_STYLE),
    (&[".ts", ".mts", ".cts"], "TypeScript", &COMMENT_C_STYLE),
    (&[".tsx", ".jsx"], "TSX/JSX", &COMMENT_C_STYLE),
    (&[".go"], "Go", &COMMENT_C_STYLE),
    (&[".swift"], "Swift", &COMMENT_C_STYLE),
    (&[".kt", ".kts"], "Kotlin", &COMMENT_C_STYLE),
    (&[".scala"], "Scala", &COMMENT_C_STYLE),
    (&[".zig"], "Zig", &COMMENT_C_STYLE),
    (&[".d"], "D", &COMMENT_C_STYLE),
    (&[".css", ".scss", ".less"], "CSS", &COMMENT_C_STYLE),
    (&[".proto"], "Protobuf", &COMMENT_C_STYLE),
    (&[".py", ".pyi"], "Python", &COMMENT_HASH),
    (&[".rb"], "Ruby", &COMMENT_HASH),
    (&[".sh", ".bash", ".zsh"], "Shell", &COMMENT_HASH),
    (&[".pl", ".pm"], "Perl", &COMMENT_HASH),
    (&[".r", ".R"], "R", &COMMENT_HASH),
    (&[".nim"], "Nim", &COMMENT_HASH),
    (&[".jl"], "Julia", &COMMENT_HASH),
    (&[".toml"], "TOML", &COMMENT_HASH),
    (&[".yaml", ".yml"], "YAML", &COMMENT_HASH),
    (&[".php"], "PHP", &COMMENT_C_STYLE),
    (&[".lua"], "Lua", &COMMENT_DASH),
    (&[".sql"], "SQL", &COMMENT_DASH),
    (&[".hs"], "Haskell", &COMMENT_DASH),
    (&[".ex", ".exs"], "Elixir", &COMMENT_HASH),
    (&[".erl", ".hrl"], "Erlang", &COMMENT_PERCENT),
    (&[".tex", ".sty"], "LaTeX", &COMMENT_PERCENT),
    (&[".el", ".lisp", ".cl", ".scm"], "Lisp", &COMMENT_SEMICOLON),
    (&[".html", ".htm"], "HTML", &COMMENT_HTML),
    (&[".xml", ".xsl", ".xsd", ".svg"], "XML", &COMMENT_HTML),
    (&[".vue"], "Vue", &COMMENT_HTML),
    (&[".md", ".markdown"], "Markdown", &COMMENT_NONE),
    (&[".json"], "JSON", &COMMENT_NONE),
    (&[".tera"], "Tera", &COMMENT_C_STYLE),
    (&[".mako", ".mak"], "Mako", &COMMENT_HASH),
    (&[".cmake"], "CMake", &COMMENT_HASH),
    (&[".gradle"], "Gradle", &COMMENT_C_STYLE),
    // Matched by exact file name: these have no extension.
    (&["Makefile"], "Makefile", &COMMENT_HASH),
    (&["Dockerfile"], "Dockerfile", &COMMENT_HASH),
    (&["CMakeLists.txt"], "CMake", &COMMENT_HASH),
    (
        &["Vagrantfile", "Rakefile", "Gemfile"],
        "Ruby",
        &COMMENT_HASH,
    ),
];

fn language_map() -> HashMap<&'static str, LanguageInfo> {
    let mut m = HashMap::new();
    for (keys, name, comment) in LANGUAGES {
        for key in *keys {
            m.insert(*key, LanguageInfo { name, comment });
        }
    }
    m
}

struct LineCounts {
    blank: usize,
    comment: usize,
    code: usize,
}

/// Count the lines of one file. A file that cannot be read is an error: it
/// used to be counted as a file with no lines, which understated the totals
/// without a word. Bytes that are not UTF-8 are replaced, not rejected, so
/// a Latin-1 source is still counted.
fn count_lines(path: &Path, comment: &CommentStyle) -> Result<LineCounts> {
    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let content = String::from_utf8_lossy(&bytes);

    let mut blank = 0usize;
    let mut comment_lines = 0usize;
    let mut code = 0usize;
    let mut in_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            blank += 1;
            continue;
        }

        if in_block {
            comment_lines += 1;
            if let Some(end) = comment.block_end
                && trimmed.contains(end)
            {
                in_block = false;
            }
            continue;
        }

        // Check single-line comments
        if comment
            .single
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            comment_lines += 1;
            continue;
        }

        // Check block comment start
        if let Some(start) = comment.block_start
            && let Some(rest) = trimmed.strip_prefix(start)
        {
            comment_lines += 1;
            if let Some(end) = comment.block_end
                && !rest.contains(end)
            {
                in_block = true;
            }
            continue;
        }

        code += 1;
    }

    Ok(LineCounts {
        blank,
        comment: comment_lines,
        code,
    })
}

struct LanguageStats {
    files: usize,
    blank: usize,
    comment: usize,
    code: usize,
}

/// The basic COCOMO estimate (organic mode) for `total_code` lines, when asked for.
fn cocomo_estimate(cocomo: bool, total_code: usize, salary: u64) -> Option<SlocCocomoEstimate> {
    if cocomo {
        let ksloc = crate::numeric::usize_to_f64(total_code) / 1000.0;
        let effort = 2.4 * ksloc.powf(1.05);
        let schedule = 2.5 * effort.powf(0.38);
        let people = if schedule > 0.0 {
            effort / schedule
        } else {
            0.0
        };
        let cost = effort * (crate::numeric::u64_to_f64(salary) / 12.0);
        Some(SlocCocomoEstimate {
            effort_person_months: effort,
            schedule_months: schedule,
            people,
            cost,
            salary,
        })
    } else {
        None
    }
}

pub fn run_sloc(file_index: &FileIndex, cocomo: bool, salary: u64) -> Result<()> {
    let lang_map = language_map();
    let mut stats: HashMap<&str, LanguageStats> = HashMap::new();

    for path in file_index.files() {
        // Try extension first, then exact filename
        let info = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                let dot_ext = format!(".{ext}");
                // We need to look up by the dot-prefixed extension
                // but our map keys include the dot already
                lang_map.get(dot_ext.as_str())
            })
            .or_else(|| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|name| lang_map.get(name))
            });

        let Some(info) = info else { continue };

        let counts = count_lines(path, info.comment)?;

        let entry = stats.entry(info.name).or_insert(LanguageStats {
            files: 0,
            blank: 0,
            comment: 0,
            code: 0,
        });
        entry.files += 1;
        entry.blank += counts.blank;
        entry.comment += counts.comment;
        entry.code += counts.code;
    }

    // Sort by code lines descending, then by name: `stats` is a HashMap, so
    // languages with equal counts would otherwise change places between runs.
    let mut sorted: Vec<(&str, &LanguageStats)> = stats.iter().map(|(k, v)| (*k, v)).collect();
    sorted.sort_by_key(|(name, stats)| (std::cmp::Reverse(stats.code), *name));

    // Totals
    let total_files: usize = sorted.iter().map(|(_, s)| s.files).sum();
    let total_blank: usize = sorted.iter().map(|(_, s)| s.blank).sum();
    let total_comment: usize = sorted.iter().map(|(_, s)| s.comment).sum();
    let total_code: usize = sorted.iter().map(|(_, s)| s.code).sum();

    // COCOMO
    let cocomo_estimate = cocomo_estimate(cocomo, total_code, salary);

    if json_output::is_json_mode() {
        let output = SlocOutput {
            languages: sorted
                .iter()
                .map(|(name, s)| SlocLanguageEntry {
                    language: name.to_string(),
                    files: s.files,
                    blank: s.blank,
                    comment: s.comment,
                    code: s.code,
                })
                .collect(),
            total: SlocTotals {
                files: total_files,
                blank: total_blank,
                comment: total_comment,
                code: total_code,
            },
            cocomo: cocomo_estimate,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let rows: Vec<Vec<String>> = sorted
            .iter()
            .map(|(name, s)| {
                vec![
                    name.to_string(),
                    s.files.to_string(),
                    s.blank.to_string(),
                    s.comment.to_string(),
                    s.code.to_string(),
                ]
            })
            .collect();
        let total = vec![
            "Total".to_string(),
            total_files.to_string(),
            total_blank.to_string(),
            total_comment.to_string(),
            total_code.to_string(),
        ];
        tables::print_with_total(&tables::defs::SLOC, false, &rows, &total);

        if let Some(ref est) = cocomo_estimate {
            println!();
            println!("COCOMO Estimation (Basic Organic):");
            println!("  Effort:   {:.1} person-months", est.effort_person_months);
            println!("  Schedule: {:.1} months", est.schedule_months);
            println!("  People:   {:.1}", est.people);
            println!(
                "  Cost:     ${:.0} (at ${}/yr salary)",
                est.cost, est.salary
            );
        }
    }

    Ok(())
}
