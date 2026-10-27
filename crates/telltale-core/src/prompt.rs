//! Renders an idea as a self-contained task prompt for a coding agent.
//! The prompt is rendered at read time, so it always reflects the idea's
//! current KPI values and status.

use std::fmt::Write;

use crate::model::{Idea, Kpi};

pub struct PromptContext<'a> {
    pub product: &'a str,
    pub kpi: Option<&'a Kpi>,
    /// Current value of the linked KPI over the default window.
    pub kpi_value: Option<f64>,
}

pub fn render(idea: &Idea, ctx: &PromptContext) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Task: {}\n", idea.title);
    let _ = writeln!(
        s,
        "You are working on {}. Telltale, a behaviour-analytics agent, analysed real user sessions and found the problem below. \
         Make the change in this codebase.\n",
        ctx.product
    );

    let _ = writeln!(s, "## Problem\n\n{}\n", idea.problem);

    if !idea.evidence.is_empty() {
        let _ = writeln!(s, "## Evidence from user behaviour\n");
        for e in &idea.evidence {
            let _ = writeln!(s, "- {e}");
        }
        s.push('\n');
    }

    if let Some(kpi) = ctx.kpi {
        let _ = writeln!(s, "## Goal\n");
        let _ = write!(
            s,
            "This change should {} the KPI **{}** ({})",
            kpi.direction.as_str(),
            kpi.name,
            if kpi.description.is_empty() { kpi.definition.describe() } else { kpi.description.clone() },
        );
        if let Some(v) = ctx.kpi_value {
            let _ = write!(s, ". It is currently {}", kpi.definition.format_value(v));
        }
        if let Some(t) = kpi.target {
            let _ = write!(s, " and the target is {}", kpi.definition.format_value(t));
        }
        s.push_str(".\n");
        if !idea.expected_impact.is_empty() {
            let _ = writeln!(s, "\nExpected impact: {}", idea.expected_impact);
        }
        s.push('\n');
    } else if !idea.expected_impact.is_empty() {
        let _ = writeln!(s, "## Expected impact\n\n{}\n", idea.expected_impact);
    }

    let _ = writeln!(s, "## Proposed change\n\n{}\n", idea.proposal);

    if !idea.pages.is_empty() {
        let _ = writeln!(s, "## Where to look\n");
        let _ = writeln!(
            s,
            "Affected routes: {}. Find the code that renders these routes and the elements named in the evidence before changing anything.\n",
            idea.pages.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
        );
    }

    if !idea.acceptance_criteria.is_empty() {
        let _ = writeln!(s, "## Acceptance criteria\n");
        for c in &idea.acceptance_criteria {
            let _ = writeln!(s, "- [ ] {c}");
        }
        s.push('\n');
    }

    let _ = writeln!(s, "## Constraints\n");
    let _ = writeln!(s, "- Keep the change focused on this problem. Don't refactor unrelated code.");
    let _ = writeln!(
        s,
        "- Telltale tracks elements by `id`, `data-telltale` attribute or CSS path. If you rename or move an element named in the evidence, \
         add `data-telltale=\"<name>\"` to it so the before/after comparison keeps working."
    );
    let _ = writeln!(s, "- When you're done, summarise what you changed and how to verify it by hand.\n");
    let _ = writeln!(
        s,
        "---\nTelltale idea #{}. Once this ships, mark the idea as released in Telltale so it can measure the impact.",
        idea.id
    );
    s
}
