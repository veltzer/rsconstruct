//! The `runner-label` rule: `runs-on:` labels, and labels that cannot run
//! on the same runner.

use std::collections::BTreeMap;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Job, Matrix, RawYamlValue, Str, contains_expression};
use crate::actionlint::config::{Config, glob_match};
use crate::actionlint::expr::parser::{ExprNode, Parser};
use crate::actionlint::parse::quotes;
use crate::actionlint::yaml::go_quote;

type Compat = u32;

const COMPAT_INVALID: Compat = 0;
const COMPAT_UBUNTU_2204: Compat = 1 << 1;
const COMPAT_UBUNTU_2404: Compat = 1 << 2;
const COMPAT_MACOS_140: Compat = 1 << 3;
const COMPAT_MACOS_140L: Compat = 1 << 4;
const COMPAT_MACOS_140XL: Compat = 1 << 5;
const COMPAT_MACOS_150: Compat = 1 << 6;
const COMPAT_MACOS_150_INTEL: Compat = 1 << 7;
const COMPAT_MACOS_150L: Compat = 1 << 8;
const COMPAT_MACOS_150XL: Compat = 1 << 9;
const COMPAT_MACOS_260: Compat = 1 << 10;
const COMPAT_MACOS_260_INTEL: Compat = 1 << 11;
const COMPAT_MACOS_260L: Compat = 1 << 12;
const COMPAT_MACOS_260XL: Compat = 1 << 13;
const COMPAT_WINDOWS_2022: Compat = 1 << 14;
const COMPAT_WINDOWS_2025: Compat = 1 << 15;
const COMPAT_WINDOWS_2025_VS2026: Compat = 1 << 16;
const COMPAT_WINDOWS_11_ARM: Compat = 1 << 17;

const ALL_GITHUB_HOSTED_RUNNER_LABELS: &[&str] = &[
    "windows-latest",
    "windows-latest-8-cores",
    "windows-2025",
    "windows-2025-vs2026",
    "windows-2022",
    "windows-11-arm",
    "ubuntu-slim",
    "ubuntu-latest",
    "ubuntu-latest-4-cores",
    "ubuntu-latest-8-cores",
    "ubuntu-latest-16-cores",
    "ubuntu-24.04",
    "ubuntu-24.04-arm",
    "ubuntu-22.04",
    "ubuntu-22.04-arm",
    "macos-latest",
    "macos-latest-xlarge",
    "macos-latest-large",
    "macos-26-intel",
    "macos-26-xlarge",
    "macos-26-large",
    "macos-26",
    "macos-15-intel",
    "macos-15-xlarge",
    "macos-15-large",
    "macos-15",
    "macos-14-xlarge",
    "macos-14-large",
    "macos-14",
];

const SELF_HOSTED_RUNNER_PRESET_OS_LABELS: &[&str] = &["linux", "macos", "windows"];
const SELF_HOSTED_RUNNER_PRESET_OTHER_LABELS: &[&str] = &["self-hosted", "x64", "arm", "arm64"];

const ALL_MACOS: Compat = COMPAT_MACOS_260
    | COMPAT_MACOS_260_INTEL
    | COMPAT_MACOS_260L
    | COMPAT_MACOS_260XL
    | COMPAT_MACOS_150
    | COMPAT_MACOS_150_INTEL
    | COMPAT_MACOS_150L
    | COMPAT_MACOS_150XL
    | COMPAT_MACOS_140
    | COMPAT_MACOS_140L
    | COMPAT_MACOS_140XL;

fn default_runner_compat(label: &str) -> Option<Compat> {
    Some(match label {
        "ubuntu-slim"
        | "ubuntu-latest"
        | "ubuntu-latest-4-cores"
        | "ubuntu-latest-8-cores"
        | "ubuntu-latest-16-cores"
        | "ubuntu-24.04"
        | "ubuntu-24.04-arm" => COMPAT_UBUNTU_2404,
        "ubuntu-22.04" | "ubuntu-22.04-arm" => COMPAT_UBUNTU_2204,
        "macos-latest-xlarge" | "macos-15-xlarge" => COMPAT_MACOS_150XL,
        "macos-latest-large" | "macos-15-large" => COMPAT_MACOS_150L,
        "macos-latest" | "macos-15" => COMPAT_MACOS_150,
        "macos-26-intel" => COMPAT_MACOS_260_INTEL,
        "macos-26-xlarge" => COMPAT_MACOS_260XL,
        "macos-26-large" => COMPAT_MACOS_260L,
        "macos-26" => COMPAT_MACOS_260,
        "macos-15-intel" => COMPAT_MACOS_150_INTEL,
        "macos-14-xlarge" => COMPAT_MACOS_140XL,
        "macos-14-large" => COMPAT_MACOS_140L,
        "macos-14" => COMPAT_MACOS_140,
        "windows-latest" | "windows-latest-8-cores" | "windows-2022" => COMPAT_WINDOWS_2022,
        "windows-2025" => COMPAT_WINDOWS_2025,
        "windows-2025-vs2026" => COMPAT_WINDOWS_2025_VS2026,
        "windows-11-arm" => COMPAT_WINDOWS_11_ARM,
        "linux" => COMPAT_UBUNTU_2404 | COMPAT_UBUNTU_2204,
        "macos" => ALL_MACOS,
        "windows" => {
            COMPAT_WINDOWS_2025_VS2026
                | COMPAT_WINDOWS_2025
                | COMPAT_WINDOWS_2022
                | COMPAT_WINDOWS_11_ARM
        }
        _ => return None,
    })
}

pub struct RuleRunnerLabel {
    base: RuleBase,
    /// Set while checking a job with several labels: the first label seen
    /// for each compatibility class.
    compats: Option<BTreeMap<Compat, Str>>,
    known_labels: Vec<String>,
}

impl RuleRunnerLabel {
    pub fn new(config: Option<&Config>) -> Self {
        Self {
            base: RuleBase::new("runner-label"),
            compats: None,
            known_labels: config
                .map(|c| c.self_hosted_runner_labels.clone())
                .unwrap_or_default(),
        }
    }

    fn check_label_and_conflict(&mut self, l: &Str, m: Option<&Matrix>) {
        if l.contains_expression() {
            let labels = Self::labels_in_matrix(l, m);
            let comps: Vec<Compat> = labels.iter().map(|s| self.verify_runner_label(s)).collect();
            self.check_combi_compat(comps, &labels);
            return;
        }
        let comp = self.verify_runner_label(l);
        self.check_compat(comp, l);
    }

    fn check_label(&mut self, l: &Str, m: Option<&Matrix>) {
        if l.contains_expression() {
            for s in Self::labels_in_matrix(l, m) {
                self.verify_runner_label(&s);
            }
            return;
        }
        self.verify_runner_label(l);
    }

    fn verify_runner_label(&mut self, label: &Str) -> Compat {
        let l = label.value.as_str();
        if let Some(c) = default_runner_compat(&l.to_lowercase()) {
            return c;
        }
        if SELF_HOSTED_RUNNER_PRESET_OTHER_LABELS
            .iter()
            .any(|p| p.eq_ignore_ascii_case(l))
        {
            return COMPAT_INVALID;
        }
        let known = self.known_labels.clone();
        for k in &known {
            match path_match(k, l) {
                Err(e) => {
                    self.base.error(
                        label.pos,
                        format!(
                            "label pattern {} is an invalid glob. kindly check list of labels in actionlint.yaml config file: {e}",
                            go_quote(k)
                        ),
                    );
                    return COMPAT_INVALID;
                }
                Ok(true) => return COMPAT_INVALID,
                Ok(false) => {}
            }
        }
        let mut all: Vec<&str> = Vec::new();
        all.extend(ALL_GITHUB_HOSTED_RUNNER_LABELS);
        all.extend(SELF_HOSTED_RUNNER_PRESET_OTHER_LABELS);
        all.extend(SELF_HOSTED_RUNNER_PRESET_OS_LABELS);
        all.extend(known.iter().map(String::as_str));
        self.base.error(
            label.pos,
            format!(
                "label {} is unknown. available labels are {}. if it is a custom label for self-hosted runner, set list of labels in actionlint.yaml config file",
                go_quote(l),
                quotes(&all)
            ),
        );
        COMPAT_INVALID
    }

    /// The labels a `${{ matrix.xxx }}` expression can take, from the matrix
    /// rows and `include` entries.
    fn labels_in_matrix(label: &Str, m: Option<&Matrix>) -> Vec<Str> {
        let Some(m) = m else {
            return Vec::new();
        };
        if !label.is_expression_assigned() {
            return Vec::new();
        }
        let l = label.value.trim();
        let (parsed, _) = Parser::parse(&l[3..]);
        let Ok(expr) = parsed else {
            return Vec::new();
        };
        let ExprNode::ObjectDeref { receiver, property } = expr else {
            return Vec::new();
        };
        let ExprNode::Variable { name, .. } = receiver.as_ref() else {
            return Vec::new();
        };
        if name != "matrix" {
            return Vec::new();
        }
        let mut labels = Vec::new();
        if let Some(row) = m.rows.get(&property) {
            for v in row.values.iter().flatten() {
                if let RawYamlValue::String { value, pos } = v
                    && !contains_expression(value)
                {
                    labels.push(Str {
                        value: value.clone(),
                        quoted: false,
                        pos: *pos,
                    });
                }
            }
        }
        if let Some(include) = &m.include {
            for combi in &include.combinations {
                if let Some(assign) = combi.assigns.get(&property)
                    && let RawYamlValue::String { value, pos } = &assign.value
                    && !contains_expression(value)
                {
                    labels.push(Str {
                        value: value.clone(),
                        quoted: false,
                        pos: *pos,
                    });
                }
            }
        }
        labels
    }

    fn check_conflict(&mut self, comp: Compat, label: &Str) -> bool {
        let Some(compats) = &self.compats else {
            return true;
        };
        for (c, l) in compats {
            if c & comp == 0 {
                let message = format!(
                    "label {} conflicts with label {} defined at {}. note: to run your job on each workers, use matrix",
                    go_quote(&label.value),
                    go_quote(&l.value),
                    l.pos
                );
                self.base.error(label.pos, message);
                return false;
            }
        }
        true
    }

    fn check_compat(&mut self, comp: Compat, label: &Str) {
        if comp == COMPAT_INVALID || !self.check_conflict(comp, label) {
            return;
        }
        if let Some(compats) = &mut self.compats {
            compats.entry(comp).or_insert_with(|| label.clone());
        }
    }

    fn check_combi_compat(&mut self, mut comps: Vec<Compat>, labels: &[Str]) {
        for (i, c) in comps.clone().iter().enumerate() {
            if *c != COMPAT_INVALID && !self.check_conflict(*c, &labels[i]) {
                comps[i] = COMPAT_INVALID;
            }
        }
        for (i, c) in comps.iter().enumerate() {
            if *c != COMPAT_INVALID
                && let Some(compats) = &mut self.compats
            {
                compats.entry(*c).or_insert_with(|| labels[i].clone());
            }
        }
    }
}

/// Go's `path.Match`: `*`, `?`, `[...]` and `\` escapes; a malformed
/// pattern (unterminated class) is an error.
fn path_match(pattern: &str, name: &str) -> Result<bool, String> {
    let mut depth = 0;
    let mut escaped = false;
    for c in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => depth = 0,
            _ => {}
        }
    }
    if depth > 0 || escaped {
        return Err("syntax error in pattern".to_string());
    }
    let pat: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = name.chars().collect();
    Ok(glob_match(&pat, &s))
}

impl Rule for RuleRunnerLabel {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        let Some(runs_on) = &job.runs_on else {
            return;
        };
        let m = job.strategy.as_ref().and_then(|s| s.matrix.as_ref());
        if runs_on.labels.len() == 1 {
            self.check_label(&runs_on.labels[0], m);
            return;
        }
        self.compats = Some(BTreeMap::new());
        if let Some(expr) = &runs_on.labels_expr {
            self.check_label_and_conflict(expr, m);
        } else {
            for label in &runs_on.labels {
                self.check_label_and_conflict(label, m);
            }
        }
        self.compats = None;
    }
}
