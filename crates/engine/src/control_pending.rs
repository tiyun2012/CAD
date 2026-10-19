//! What a client is told about a control request that outlasts its wait (docs/control-protocol.md,
//! #1618): the app's control server and the bridged MCP server both say whether the request
//! still changes something when it finishes, or only reads.

use serde_json::Value;

/// Control methods that only read: a timed-out one changes nothing when it finishes.
const READ_METHODS: &[&str] = &[
    "control.pending",
    "document.inspect",
    "engine.commands",
    "ui.at",
    "ui.handles",
    "ui.inspect",
    "ui.render",
    "ui.screenshot",
    "ui.selection",
    "ui.sketchToScreen",
    "ui.triad",
    "ui.worldToScreen",
];

/// Commands that only read the design (inspection, lists, measurements).
pub fn command_reads_only(id: &str) -> bool {
    let id = crate::find_command(id).map_or(id, |c| c.id);
    id.starts_with("inspect.")
        || id.starts_with("model.")
        || id.ends_with(".inspect")
        || id.ends_with(".list")
        || matches!(id, "document.inspect" | "engine.commands" | "render.status" | "render.backends" | "render.settings.get" | "file.slicers")
}

/// Whether the request `method` with `params` only reads (the design, the window or the
/// command list): renders and screenshots count, as they change nothing in the design.
pub fn reads_only(method: &str, params: &Value) -> bool {
    match method {
        "engine.execute" | "command" => params.get("command").or_else(|| params.get("id")).and_then(Value::as_str).is_some_and(command_reads_only),
        m => READ_METHODS.contains(&m),
    }
}

/// The request in a message: "`engine.execute` (solid.fillet)", or just "`ui.inspect`".
pub fn describe(method: &str, params: &Value) -> String {
    let command = matches!(method, "engine.execute" | "command")
        .then(|| params.get("command").or_else(|| params.get("id")).and_then(Value::as_str))
        .flatten()
        .filter(|c| c.len() <= 128);
    match command {
        Some(c) => format!("`{method}` ({c})"),
        None => format!("`{}`", method.chars().take(128).collect::<String>()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_are_told_from_changes() {
        for (m, p) in [
            ("ui.inspect", json!({})),
            ("document.inspect", json!({"measure": true})),
            ("engine.execute", json!({"command": "sketch.inspect"})),
            ("engine.execute", json!({"command": "inspect.measure"})),
            ("engine.execute", json!({"command": "model.edges", "params": {"body": "Body1"}})),
        ] {
            assert!(reads_only(m, &p), "{m} {p}");
        }
        for (m, p) in [
            ("engine.execute", json!({"command": "solid.fillet"})),
            ("engine.execute", json!({})),
            ("engine.batch", json!({"commands": []})),
            ("ui.click", json!({"x": 1, "y": 2})),
            ("app.quit", json!({})),
        ] {
            assert!(!reads_only(m, &p), "{m} {p}");
        }
        assert_eq!(describe("engine.execute", &json!({"command": "solid.fillet"})), "`engine.execute` (solid.fillet)");
        assert_eq!(describe("ui.inspect", &json!({})), "`ui.inspect`");
    }
}
