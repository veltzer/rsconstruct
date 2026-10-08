//! The `id` rule: naming convention and uniqueness of job and step IDs.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Job, Pos, Step, Str};
use crate::actionlint::yaml::go_quote;

static JOB_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_-]*$").expect("valid regex"));

pub struct RuleId {
    base: RuleBase,
    seen: BTreeMap<String, Pos>,
}

impl RuleId {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("id"),
            seen: BTreeMap::new(),
        }
    }

    fn validate_convention(&mut self, id: Option<&Str>, what: &str) {
        let Some(id) = id else {
            return;
        };
        if id.value.is_empty() || id.contains_expression() || JOB_ID_PATTERN.is_match(&id.value) {
            return;
        }
        self.base.error(
            id.pos,
            format!(
                "invalid {what} ID {}. {what} ID must start with a letter or _ and contain only alphanumeric characters, -, or _",
                go_quote(&id.value)
            ),
        );
    }
}

impl Rule for RuleId {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        self.seen.clear();
        self.validate_convention(Some(&job.id), "job");
        for j in &job.needs {
            self.validate_convention(Some(j), "job");
        }
    }

    fn visit_job_post(&mut self, _job: &Job) {
        self.seen.clear();
    }

    fn visit_step(&mut self, step: &Step) {
        let Some(id) = &step.id else {
            return;
        };
        self.validate_convention(Some(id), "step");
        let key = id.value.to_lowercase();
        if let Some(prev) = self.seen.get(&key) {
            self.base.error(
                id.pos,
                format!(
                    "step ID {} duplicates. previously defined at {prev}. step ID must be unique within a job. note that step ID is case insensitive",
                    go_quote(&id.value)
                ),
            );
            return;
        }
        self.seen.insert(key, id.pos);
    }
}
