use std::collections::BTreeMap;

use crate::setup::types::*;

pub(crate) fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// For the CLI and the UI: the plan as the §76 text block.
pub fn plan_text(plan: &EnvironmentPlan) -> String {
    let mut groups: BTreeMap<usize, (String, Vec<String>)> = BTreeMap::new();
    let order = ["install", "create", "configure", "start", "tunnel", "check"];
    for s in &plan.steps {
        let idx = order
            .iter()
            .position(|g| *g == s.group)
            .unwrap_or(order.len());
        let title = match s.group.as_str() {
            "install" => "Install",
            "create" => "Create",
            "configure" => "Configure",
            "start" => "Start",
            "tunnel" => "Tunnel",
            _ => "Check",
        };
        let mark = if s.done { "✓" } else { "•" };
        groups
            .entry(idx)
            .or_insert_with(|| (title.to_string(), Vec::new()))
            .1
            .push(format!(
                "  {mark} {}{}",
                s.label,
                s.note
                    .as_ref()
                    .map(|n| format!("\n      {n}"))
                    .unwrap_or_default()
            ));
    }
    let mut out = format!("Environment Plan: {}\n", plan.project_name);
    if !plan.manifest_found {
        out.push_str("(no .openlocalserver/environment.yaml; planned from what was detected)\n");
    }
    for (_, (title, lines)) in groups {
        out.push_str(&format!("\n{title}:\n{}\n", lines.join("\n")));
    }
    if !plan.conflicts.is_empty() {
        out.push_str("\nConflicts:\n");
        for c in &plan.conflicts {
            out.push_str(&format!(
                "  {} [{}] {}\n",
                if c.blocking { "✗" } else { "⚠" },
                c.kind,
                c.message
            ));
            if !c.resolution.is_empty() {
                out.push_str(&format!("      → {}\n", c.resolution));
            }
        }
    }
    out
}
