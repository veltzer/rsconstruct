//! The `credentials` rule: passwords written into `container:`/`services:`.

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Container, Job};
use crate::actionlint::yaml::go_quote;

pub struct RuleCredentials {
    base: RuleBase,
}

impl RuleCredentials {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("credentials"),
        }
    }

    fn check_container(&mut self, where_: &str, n: &Container) {
        let Some(c) = &n.credentials else {
            return;
        };
        let Some(p) = &c.password else {
            return;
        };
        if !p.is_expression_assigned() {
            self.base.error(
                p.pos,
                format!(
                    "\"password\" section in {where_} should be specified via secrets. do not put password value directly"
                ),
            );
        }
    }
}

impl Rule for RuleCredentials {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        if let Some(c) = &job.container {
            self.check_container("\"container\" section", c);
        }
        if let Some(s) = &job.services {
            for service in s.value.values() {
                let where_ = format!("{} service", go_quote(&service.name.value));
                self.check_container(&where_, &service.container);
            }
        }
    }
}
