//! The rules that check a parsed workflow, and the visitor that drives
//! them: a port of actionlint's `rule.go`, `pass.go` and `rule_*.go`.
//!
//! Every rule sees the workflow before and after its jobs, every job before
//! and after its steps, and every step, exactly as actionlint's `Visitor`
//! calls its passes.

pub mod action;
pub mod credentials;
pub mod deprecated_commands;
pub mod env_var;
pub mod events;
pub mod expression;
pub mod glob;
pub mod id;
pub mod if_cond;
pub mod job_needs;
pub mod matrix;
pub mod permissions;
pub mod runner_label;
pub mod shell_name;
pub mod workflow_call;

use super::LintError;
use super::ast::{Job, Pos, Step, Workflow};

/// What every rule carries: its name (the `[kind]` in reports) and the
/// errors it found.
#[derive(Debug, Default)]
pub struct RuleBase {
    pub name: &'static str,
    pub errs: Vec<LintError>,
}

impl RuleBase {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            errs: Vec::new(),
        }
    }

    pub fn error(&mut self, pos: Pos, message: String) {
        self.errs.push(LintError {
            message,
            line: pos.line,
            column: pos.col,
            kind: self.name,
        });
    }
}

pub trait Rule {
    fn base(&mut self) -> &mut RuleBase;
    fn visit_step(&mut self, _step: &Step) {}
    fn visit_job_pre(&mut self, _job: &Job) {}
    fn visit_job_post(&mut self, _job: &Job) {}
    fn visit_workflow_pre(&mut self, _workflow: &Workflow) {}
    fn visit_workflow_post(&mut self, _workflow: &Workflow) {}
}

/// Runs the rules over the workflow in actionlint's order and collects what
/// they report.
pub fn visit(workflow: &Workflow, rules: &mut [Box<dyn Rule + '_>]) -> Vec<LintError> {
    for rule in rules.iter_mut() {
        rule.visit_workflow_pre(workflow);
    }
    for job in workflow.jobs() {
        for rule in rules.iter_mut() {
            rule.visit_job_pre(job);
        }
        for step in job.steps.iter().flatten() {
            for rule in rules.iter_mut() {
                rule.visit_step(step);
            }
        }
        for rule in rules.iter_mut() {
            rule.visit_job_post(job);
        }
    }
    for rule in rules.iter_mut() {
        rule.visit_workflow_post(workflow);
    }
    let mut all = Vec::new();
    for rule in rules.iter_mut() {
        all.append(&mut rule.base().errs);
    }
    all
}
