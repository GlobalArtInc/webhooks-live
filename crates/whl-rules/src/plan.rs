use serde::Serialize;

use crate::kind::{StepClass, StepKind};
use crate::model::Rule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cut {
    pub rule: usize,
    pub step: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulePlan {
    cut: Option<Cut>,
}

impl RulePlan {
    pub fn of(rules: &[Rule]) -> Self {
        let cut = rules
            .iter()
            .enumerate()
            .rev()
            .find_map(|(rule_index, rule)| {
                rule.steps
                    .iter()
                    .rposition(|step| StepKind::of(&step.action).class() == StepClass::Response)
                    .map(|step| Cut {
                        rule: rule_index,
                        step,
                    })
            });
        Self { cut }
    }

    pub fn cut(&self) -> Option<Cut> {
        self.cut
    }

    pub fn has_before_part(&self) -> bool {
        self.cut.is_some()
    }

    pub fn is_after(&self, rule: usize, step: usize) -> bool {
        match self.cut {
            Some(cut) => (rule, step) > (cut.rule, cut.step),
            None => true,
        }
    }

    pub fn is_before(&self, rule: usize, step: usize) -> bool {
        !self.is_after(rule, step)
    }
}
