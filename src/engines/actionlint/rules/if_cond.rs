//! The `if-cond` rule: conditions that are always true or always false.

use super::{Rule, RuleBase};
use crate::engines::actionlint::ast::{Job, Pos, Step, Str};
use crate::engines::actionlint::expr::parser::Parser;
use crate::engines::actionlint::expr::sema::SemanticsChecker;
use crate::engines::actionlint::yaml::go_quote;

pub struct RuleIfCond {
    base: RuleBase,
}

impl RuleIfCond {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("if-cond"),
        }
    }

    fn check_if_cond(&mut self, n: Option<&Str>) {
        let Some(n) = n else {
            return;
        };
        match (n.value.find("${{"), n.value.find("}}")) {
            (Some(s), Some(e)) => self.check_placeholder(n, s, e),
            _ => self.check_expression(n.pos, &n.value),
        }
    }

    fn check_placeholder(&mut self, n: &Str, start: usize, end: usize) {
        if start > 0 || end + 2 < n.value.len() || n.value.matches("${{").count() > 1 {
            self.base.error(
                n.pos,
                format!(
                    "if: condition {} is always evaluated to true because extra characters are around ${{{{ }}}}",
                    go_quote(&n.value)
                ),
            );
            return;
        }
        let inner = n.value[start + 3..end].to_string();
        self.check_expression(n.pos, &inner);
    }

    fn check_expression(&mut self, pos: Pos, input: &str) {
        let i = input.trim();
        let (parsed, _) = Parser::parse(&format!("{i}}}}}"));
        if let Ok(e) = parsed
            && SemanticsChecker::is_constant(&e)
        {
            self.base.error(
                pos,
                format!(
                    "constant expression {} in condition. remove the if: section",
                    go_quote(i)
                ),
            );
        }
    }
}

impl Rule for RuleIfCond {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_step(&mut self, step: &Step) {
        self.check_if_cond(step.if_cond.as_ref());
    }

    fn visit_job_pre(&mut self, job: &Job) {
        self.check_if_cond(job.if_cond.as_ref());
        if let Some(s) = &job.snapshot {
            self.check_if_cond(s.if_cond.as_ref());
        }
    }
}
