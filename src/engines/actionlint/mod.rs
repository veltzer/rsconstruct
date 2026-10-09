//! A port of actionlint (v1.7.12), the GitHub Actions workflow linter, as
//! the engine behind `processor.checker.iactionlint`.
//!
//! The port follows actionlint's source file by file: `yaml` and `parse`
//! build the workflow syntax tree (`ast`) and report `syntax-check` errors;
//! `expr` lexes, parses and type-checks `${{ }}` expressions; `rules` holds
//! the checks that walk the tree. Messages, positions and rule names are
//! actionlint's, so a report reads the same from either tool.
//!
//! Not ported: the `shellcheck` and `pyflakes` rules, which run external
//! tools over `run:` scripts.

pub mod action_metadata;
pub mod ast;
pub mod availability;
pub mod config;
pub mod cron;
pub mod expr;
pub mod parse;
pub mod reusable_workflow;
pub mod rules;
#[cfg(test)]
mod tests;
pub mod yaml;

use std::fmt;
use std::path::PathBuf;

pub use config::Config;

use action_metadata::LocalActionsCache;
use reusable_workflow::LocalReusableWorkflowCache;
use rules::Rule;

/// One problem, as actionlint reports it: `path:line:col: message [kind]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintError {
    pub message: String,
    /// 1-based (0 when unknown, as actionlint prints for some YAML errors).
    pub line: u32,
    /// 1-based (0 when unknown).
    pub column: u32,
    /// The rule name: `syntax-check`, `expression`, `events`, ...
    pub kind: &'static str,
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} [{}]",
            self.line, self.column, self.message, self.kind
        )
    }
}

/// actionlint's ordering: by line, then column, then message; equal errors
/// (which aliases can duplicate) are reported once.
pub fn sort_and_dedup(errors: &mut Vec<LintError>) {
    errors.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column.cmp(&b.column))
            .then(a.message.cmp(&b.message))
    });
    errors.dedup_by(|a, b| a.line == b.line && a.column == b.column && a.message == b.message);
}

/// Lints the workflow files of one repository. Local actions and reusable
/// workflows are read once and shared between files, as actionlint's
/// per-project caches do.
pub struct Linter {
    config: Option<Config>,
    actions: LocalActionsCache,
    workflows: LocalReusableWorkflowCache,
}

impl Linter {
    /// `root` is the repository root (where `./.github/actions/...` resolve);
    /// `cwd` is what workflow paths given to `lint` are relative to.
    pub fn new(root: Option<PathBuf>, cwd: PathBuf, config: Option<Config>) -> Self {
        Self {
            config,
            actions: LocalActionsCache::new(root.clone()),
            workflows: LocalReusableWorkflowCache::new(root, cwd),
        }
    }

    /// Checks one workflow file. `path` is how the file is named in reports
    /// and matched against the config's `paths`.
    pub fn lint(&self, path: &str, source: &[u8]) -> Vec<LintError> {
        let (workflow, mut all) = parse::parse(source);
        if let Some(w) = workflow {
            let config = self.config.as_ref();
            let mut rules: Vec<Box<dyn Rule + '_>> = vec![
                Box::new(rules::matrix::RuleMatrix::new()),
                Box::new(rules::credentials::RuleCredentials::new()),
                Box::new(rules::shell_name::RuleShellName::new()),
                Box::new(rules::runner_label::RuleRunnerLabel::new(config)),
                Box::new(rules::events::RuleEvents::new()),
                Box::new(rules::job_needs::RuleJobNeeds::new()),
                Box::new(rules::action::RuleAction::new(&self.actions)),
                Box::new(rules::env_var::RuleEnvVar::new()),
                Box::new(rules::id::RuleId::new()),
                Box::new(rules::glob::RuleGlob::new()),
                Box::new(rules::permissions::RulePermissions::new()),
                Box::new(rules::workflow_call::RuleWorkflowCall::new(
                    path,
                    &self.workflows,
                )),
                Box::new(rules::expression::RuleExpression::new(
                    &self.actions,
                    &self.workflows,
                    config,
                )),
                Box::new(rules::deprecated_commands::RuleDeprecatedCommands::new()),
                Box::new(rules::if_cond::RuleIfCond::new()),
            ];
            all.extend(rules::visit(&w, &mut rules));
        }
        if let Some(config) = &self.config {
            all = config.filter_errors(path, all);
        }
        sort_and_dedup(&mut all);
        all
    }
}
