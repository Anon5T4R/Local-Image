//! The commands Local Image offers an agent.
//!
//! The list is the viewer's own: the palette's commands (`src/shell.rs`) plus the two actions the
//! app has outside it — opening one file (the path `Abrir com` and the command line pass) and the
//! image summary the footer shows. Nothing here is a new capability: it is what the app does,
//! named so an agent can reach it.
//!
//! A command with `appOnly` runs only in the window (the viewport, the dialogs); the headless
//! session refuses it and points at `--control`/`--connect`.

use serde_json::{Map, Value, json};

/// One command of the app.
pub struct Spec {
    /// The id the palette and the control channel use.
    pub id: &'static str,
    /// The English label the palette shows (the i18n key).
    pub label: &'static str,
    /// The menu it is listed under.
    pub menu: &'static str,
    /// The window is needed: the viewport, a dialog, the updater.
    pub app_only: bool,
}

/// Every command, in the palette's order.
pub const SPECS: &[Spec] = &[
    Spec { id: "file.open", label: "Open…", menu: "File", app_only: false },
    Spec { id: "file.open_folder", label: "Open folder…", menu: "File", app_only: false },
    Spec { id: "file.export_clean", label: "Export clean", menu: "File", app_only: false },
    Spec { id: "view.previous", label: "Previous image", menu: "View", app_only: false },
    Spec { id: "view.next", label: "Next image", menu: "View", app_only: false },
    Spec { id: "view.zoom_in", label: "Zoom in", menu: "View", app_only: true },
    Spec { id: "view.zoom_out", label: "Zoom out", menu: "View", app_only: true },
    Spec { id: "view.fit", label: "Fit to window", menu: "View", app_only: true },
    Spec { id: "view.center", label: "Center image", menu: "View", app_only: true },
    Spec { id: "view.rotate_left", label: "Rotate left", menu: "View", app_only: false },
    Spec { id: "view.rotate_right", label: "Rotate right", menu: "View", app_only: false },
    Spec { id: "view.fullscreen", label: "Full screen", menu: "View", app_only: true },
    Spec { id: "view.resize_window", label: "Resize window to image", menu: "View", app_only: true },
    Spec { id: "view.exif", label: "EXIF panel", menu: "View", app_only: false },
    Spec { id: "image.info", label: "Image information", menu: "View", app_only: false },
    Spec { id: "settings.open", label: "Settings", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.system", label: "Theme: System", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.light", label: "Theme: Light", menu: "Settings", app_only: true },
    Spec { id: "settings.theme.dark", label: "Theme: Dark", menu: "Settings", app_only: true },
    Spec { id: "help.updates", label: "Check for Updates…", menu: "Help", app_only: true },
    Spec { id: "help.about", label: "About Local Image", menu: "Help", app_only: true },
    Spec { id: "app.exit", label: "Exit", menu: "Local Image", app_only: true },
];

/// The spec of `id`, if the app has that command.
pub fn spec(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// Does the app have the command `id`?
pub fn has(id: &str) -> bool {
    spec(id).is_some()
}

/// The parameters of a command as a JSON Schema. Only the four that take any are named; every
/// other command takes no parameters.
pub fn params(id: &str) -> Value {
    let path = |what: &str| json!({"type": "string", "description": what});
    let only = |props: Value| json!({"type": "object", "properties": props, "additionalProperties": false});
    match id {
        "file.open" => {
            let mut o = only(json!({"path": path("Image file to open, as `Abrir com` passes it")}));
            o["required"] = json!(["path"]);
            o
        }
        "file.open_folder" => {
            let mut o = only(json!({"path": path("Folder whose images and videos the viewer lists")}));
            o["required"] = json!(["path"]);
            o
        }
        "file.export_clean" => only(json!({
            "to": path("Destination file (default: `<name>.clean.png` next to the original)")
        })),
        "image.info" => only(json!({"path": path("Image to describe (default: the one open)")})),
        _ => only(json!({})),
    }
}

/// The catalogue as `command_list` and `commands` return it, in palette order.
pub fn list() -> Vec<Value> {
    SPECS.iter().map(entry).collect()
}

fn entry(s: &'static Spec) -> Value {
    json!({"id": s.id, "label": s.label, "menu": s.menu, "appOnly": s.app_only, "params": params(s.id)})
}

/// The catalogue kept by `filter` (case-insensitive id, label or menu substring), and only the
/// ids and labels when `brief`. An empty `filter` keeps everything.
pub fn search(filter: Option<&str>, brief: bool) -> Vec<Value> {
    let needle = filter.map(str::to_lowercase).filter(|n| !n.is_empty());
    list()
        .into_iter()
        .filter(|c| match &needle {
            None => true,
            Some(n) => ["id", "label", "menu"].iter().any(|k| c.get(*k).and_then(Value::as_str).is_some_and(|v| v.to_lowercase().contains(n))),
        })
        .map(|c| {
            if brief {
                match c {
                    Value::Object(mut o) => {
                        o.retain(|k, _| k == "id" || k == "label");
                        Value::Object(o)
                    }
                    other => other,
                }
            } else {
                c
            }
        })
        .collect()
}

/// The parameters of one command as an object, refusing a key the command does not read: a typo
/// must not run a valid command with the parameter silently dropped. No parameters at all is an
/// empty object.
pub fn arguments(id: &str, params: &Value) -> Result<Map<String, Value>, String> {
    let map = match params {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(format!("`{id}`: params must be a JSON object")),
    };
    let schema = crate::automation::catalogue::params(id);
    let allowed: Vec<&str> = schema
        .as_object()
        .and_then(|o| o.get("properties"))
        .and_then(Value::as_object)
        .map(|p| p.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        let expected = if allowed.is_empty() { "it takes no parameters".to_string() } else { format!("its parameters are {}", allowed.join(", ")) };
        return Err(format!("`{id}`: unknown parameter `{key}`: {expected}"));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_command_is_listed_once() {
        let mut ids: Vec<&str> = SPECS.iter().map(|s| s.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "a command id is listed twice");
        assert!(has("file.open_folder") && has("view.exif") && !has("view.nope"));
        assert!(spec("view.zoom_in").is_some_and(|s| s.app_only) && !spec("file.export_clean").is_some_and(|s| s.app_only));
    }

    #[test]
    fn search_filters_by_id_label_or_menu_and_brief_keeps_the_names() {
        let all = search(None, false);
        assert_eq!(all.len(), SPECS.len());
        let theme = search(Some("THEME"), false);
        assert_eq!(theme.len(), 3, "{theme:?}");
        let view = search(Some("View"), false);
        assert!(view.iter().all(|c| c["menu"] == "View"));
        let brief = search(Some("export"), true);
        assert_eq!(brief, vec![json!({"id": "file.export_clean", "label": "Export clean"})]);
        assert!(search(Some("zzz"), false).is_empty());
        assert_eq!(search(Some(""), false).len(), SPECS.len(), "an empty filter keeps everything");
    }

    #[test]
    fn parameters_are_checked_against_the_schema() {
        let ok = arguments("file.open", &json!({"path": "a.png"})).expect("a known key");
        assert_eq!(ok.len(), 1);
        assert!(arguments("file.open", &json!({})).is_ok());
        let e = arguments("file.open", &json!({"pth": "a.png"})).expect_err("a typo is refused");
        assert!(e.contains("unknown parameter `pth`") && e.contains("path"), "{e}");
        let e = arguments("view.next", &json!({"path": "a.png"})).expect_err("view.next takes none");
        assert!(e.contains("unknown parameter") && e.contains("no parameters"), "{e}");
        assert!(arguments("view.next", &Value::Null).is_ok(), "no params at all is fine");
        assert!(arguments("view.next", &json!([1])).is_err());
    }
}
