//! The `matrix` rule: duplicate values and `exclude` entries that match
//! nothing.

use std::collections::BTreeMap;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Job, Matrix, MatrixRow, RawYamlValue, contains_expression};
use crate::actionlint::parse::sorted_quotes;
use crate::actionlint::yaml::go_quote;

pub struct RuleMatrix {
    base: RuleBase,
}

impl RuleMatrix {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("matrix"),
        }
    }

    fn check_duplicate_in_row(&mut self, row: &MatrixRow) {
        let Some(values) = &row.values else {
            return;
        };
        let name = row.name.as_ref().map_or("", |n| n.value.as_str());
        let mut seen: Vec<&RawYamlValue> = Vec::new();
        for v in values {
            if let Some(p) = seen.iter().find(|p| p.equals(v)) {
                self.base.error(
                    v.pos(),
                    format!(
                        "duplicate value {v} is found in matrix {}. the same value is at {}",
                        go_quote(name),
                        p.pos()
                    ),
                );
                continue;
            }
            seen.push(v);
        }
    }

    fn check_exclude(&mut self, m: &Matrix) {
        let Some(exclude) = &m.exclude else {
            return;
        };
        if exclude.combinations.is_empty()
            || m.include
                .as_ref()
                .is_some_and(super::super::ast::MatrixCombinations::contains_expression)
        {
            return;
        }
        if m.rows.is_empty() && m.include.as_ref().is_none_or(|i| i.combinations.is_empty()) {
            self.base.error(
                m.pos,
                "\"exclude\" section exists but no matrix variation exists".to_string(),
            );
            return;
        }
        let mut rows: BTreeMap<String, Vec<RawYamlValue>> = BTreeMap::new();
        let mut ignored: Vec<&String> = Vec::new();
        for (n, r) in &m.rows {
            match &r.values {
                None => ignored.push(n),
                Some(values) => {
                    rows.insert(n.clone(), values.clone());
                }
            }
        }
        if let Some(include) = &m.include {
            for c in &include.combinations {
                for (n, a) in &c.assigns {
                    if ignored.contains(&n) {
                        continue;
                    }
                    let row = rows.entry(n.clone()).or_default();
                    if row.iter().any(|v| v.equals(&a.value)) {
                        continue;
                    }
                    row.push(a.value.clone());
                }
            }
        }
        for c in &exclude.combinations {
            for (k, a) in &c.assigns {
                if ignored.contains(&k) {
                    continue;
                }
                let Some(row) = rows.get(k) else {
                    let names: Vec<&String> = rows.keys().collect();
                    self.base.error(
                        a.key.pos,
                        format!(
                            "{} in \"exclude\" section does not exist in matrix. available matrix configurations are {}",
                            go_quote(k),
                            sorted_quotes(&names)
                        ),
                    );
                    continue;
                };
                if row.iter().any(|v| is_yaml_value_subset(v, &a.value)) {
                    continue;
                }
                let possible: Vec<String> = row.iter().map(ToString::to_string).collect();
                self.base.error(
                    a.value.pos(),
                    format!(
                        "value {} in \"exclude\" does not match in matrix {} combinations. possible values are {}",
                        a.value,
                        go_quote(k),
                        possible.join(", ")
                    ),
                );
            }
        }
    }
}

fn is_yaml_value_subset(v: &RawYamlValue, sub: &RawYamlValue) -> bool {
    if let RawYamlValue::String { value, .. } = sub
        && contains_expression(value)
    {
        return true;
    }
    match v {
        RawYamlValue::Object { props, .. } => {
            let RawYamlValue::Object {
                props: sub_props, ..
            } = sub
            else {
                return false;
            };
            sub_props
                .iter()
                .all(|(n, s)| props.get(n).is_some_and(|p| is_yaml_value_subset(p, s)))
        }
        RawYamlValue::Array { elems, .. } => {
            let RawYamlValue::Array {
                elems: sub_elems, ..
            } = sub
            else {
                return false;
            };
            elems.len() == sub_elems.len()
                && elems
                    .iter()
                    .zip(sub_elems)
                    .all(|(a, b)| is_yaml_value_subset(a, b))
        }
        RawYamlValue::String { value, .. } => {
            if contains_expression(value) {
                return true;
            }
            v.equals(sub)
        }
    }
}

impl Rule for RuleMatrix {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        let Some(strategy) = &job.strategy else {
            return;
        };
        let Some(m) = &strategy.matrix else {
            return;
        };
        if m.expression.is_some() {
            return;
        }
        for row in m.rows.values() {
            self.check_duplicate_in_row(row);
        }
        self.check_exclude(m);
    }
}
