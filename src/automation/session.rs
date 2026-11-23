//! Running Local Image's commands without a window: the viewer's state (the folder, the position
//! and the rotation) and one function per command.
//!
//! The same functions serve the headless session ([`Session`], behind `local-image mcp`,
//! `run --script` and `commands`) and the running app's control channel (`--control PORT`): the
//! app implements [`Viewer`] over its own fields and adds the window's side effects (decoding, the
//! footer) after each call. One implementation, so `mcp --connect` and the headless server answer
//! alike.
//!
//! **Deny by default.** With `--automation-read-root`/`--automation-write-root`, every file a
//! command touches goes through [`craft_mcp::automation::Roots`]: a folder that is not under a
//! read root cannot be listed or opened, and a destination that is not under a write root cannot
//! be written. No roots at all means no confinement (the user's own app).

use std::path::{Path, PathBuf};

use craft_mcp::automation::{Access, Roots};
use serde_json::{Value, json};

use super::catalogue;
use crate::img;

/// Most files `file.open_folder` returns (the folder may hold more; `count` says how many).
pub const MAX_LISTED: usize = 1000;

/// The viewer's state a command reads and writes: the folder's media, the current position in it
/// and the rotation the viewer shows it with.
pub trait Viewer {
    fn files(&self) -> &[PathBuf];
    fn index(&self) -> Option<usize>;
    fn set_files(&mut self, files: Vec<PathBuf>);
    fn set_index(&mut self, index: Option<usize>);
    /// Quarter turns clockwise, as `[`/`]` and the rotate buttons leave it.
    fn rotation(&self) -> u32;
    fn set_rotation(&mut self, quarters: u32);
}

/// The headless viewer: what a session that no window drives remembers between commands.
#[derive(Debug, Default)]
pub struct Headless {
    files: Vec<PathBuf>,
    index: Option<usize>,
    rotation: u32,
}

impl Viewer for Headless {
    fn files(&self) -> &[PathBuf] {
        &self.files
    }
    fn index(&self) -> Option<usize> {
        self.index
    }
    fn set_files(&mut self, files: Vec<PathBuf>) {
        self.files = files;
    }
    fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
    }
    fn rotation(&self) -> u32 {
        self.rotation
    }
    fn set_rotation(&mut self, quarters: u32) {
        self.rotation = quarters % 4;
    }
}

/// A headless session: the viewer state, the automation roots and the commands on top of them.
#[derive(Debug, Default)]
pub struct Session {
    viewer: Headless,
    roots: Option<Roots>,
}

impl Session {
    pub fn new(roots: Option<Roots>) -> Self {
        Self { viewer: Headless::default(), roots }
    }

    /// Where this session's calls go, for the MCP server's instructions.
    pub fn describe(&self) -> String {
        match &self.roots {
            None => "headless".to_string(),
            Some(roots) => format!("headless, files confined: {}", roots.describe()),
        }
    }

    /// One control-channel method, as the running app answers them: `command.execute`,
    /// `command.list` and `app.info`.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "command.execute" => {
                let command = params.get("command").and_then(Value::as_str).ok_or("command.execute: missing string `command`")?;
                let args = params.get("params").cloned().unwrap_or(Value::Null);
                self.exec(command, &args)
            }
            "command.list" => {
                let filter = params.get("filter").and_then(Value::as_str);
                let brief = params.get("brief").and_then(Value::as_bool).unwrap_or(false);
                Ok(Value::Array(catalogue::search(filter, brief)))
            }
            "app.info" => Ok(json!({
                "id": "local-image",
                "name": "Local Image",
                "version": env!("CARGO_PKG_VERSION"),
                "state": state(&self.viewer),
            })),
            other => Err(format!("unknown method `{other}`: this app answers command.execute, command.list and app.info")),
        }
    }

    /// Run one command by id.
    pub fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let Some(spec) = catalogue::spec(id) else {
            return Err(format!("unknown command `{id}`: command_list lists what this app has"));
        };
        if spec.app_only {
            return Err(format!(
                "`{id}` runs in the window: start the app with `local-image --control PORT` and use `mcp --connect PORT`, or run it from the palette"
            ));
        }
        let args = catalogue::arguments(id, params)?;
        let text =
            |key: &str| -> Result<&str, String> { args.get(key).and_then(Value::as_str).ok_or_else(|| format!("`{id}`: `{key}` must be a string")) };
        match id {
            "file.open" => open_file(&mut self.viewer, self.roots.as_ref(), text("path")?),
            "file.open_folder" => open_folder(&mut self.viewer, self.roots.as_ref(), text("path")?),
            "file.export_clean" => export_clean(&self.viewer, self.roots.as_ref(), args.get("to").and_then(Value::as_str)),
            "image.info" => info(&self.viewer, self.roots.as_ref(), args.get("path").and_then(Value::as_str)),
            "view.previous" => step(&mut self.viewer, -1),
            "view.next" => step(&mut self.viewer, 1),
            "view.rotate_left" => rotate(&mut self.viewer, 3),
            "view.rotate_right" => rotate(&mut self.viewer, 1),
            "view.exif" => exif(&self.viewer),
            // Every other command is `appOnly` and was refused above; a new one lands here until
            // its arm exists, which the "every command runs somewhere" test catches.
            _ => Err(format!("`{id}` has no headless implementation")),
        }
    }
}

/// The state a reply describes: the folder, the position and the rotation.
pub fn state(viewer: &impl Viewer) -> Value {
    let files = viewer.files();
    let index = viewer.index().filter(|i| *i < files.len());
    json!({
        "folder": files.first().and_then(|f| f.parent()).map(display),
        "path": index.and_then(|i| files.get(i)).map(|p| display(p)),
        "index": index,
        "position": index.map(|i| i + 1),
        "count": files.len(),
        "rotation": viewer.rotation() % 4 * 90,
    })
}

/// Open one media file (`file.open`): lists its folder and points the viewer at it.
pub fn open_file(viewer: &mut impl Viewer, roots: Option<&Roots>, path: &str) -> Result<Value, String> {
    let asked = crate::img::sanitize_path_input(path);
    check(roots, &asked, Access::Read)?;
    let real = resolve(&asked)?;
    if real.is_dir() {
        return Err(format!("`{}` is a folder: use `file.open_folder`", display(&real)));
    }
    if !img::is_image(&real) && !img::is_video(&real) {
        return Err(format!("`{}` is not an image or a video this viewer lists", display(&real)));
    }
    let Some(parent) = real.parent() else { return Err(format!("`{}` has no folder", display(&real))) };
    let files = img::list_dir(&display(parent))?;
    let index = files.iter().position(|f| f.file_name() == real.file_name());
    let Some(index) = index else { return Err(format!("`{}` is not in the folder's listing", display(&real))) };
    let kind = if img::is_image(&real) { "image" } else { "video" };
    viewer.set_files(files);
    viewer.set_index(Some(index));
    viewer.set_rotation(0);
    Ok(json!({"path": display(&real), "kind": kind, "state": state(viewer)}))
}

/// Open a folder (`file.open_folder`): lists its media and shows the first.
pub fn open_folder(viewer: &mut impl Viewer, roots: Option<&Roots>, path: &str) -> Result<Value, String> {
    let asked = crate::img::sanitize_path_input(path);
    check(roots, &asked, Access::Read)?;
    let real = resolve(&asked)?;
    if !real.is_dir() {
        return Err(format!("`{}` is not a folder: use `file.open` for a file", display(&real)));
    }
    let files = img::list_dir(&display(&real))?;
    let folder = display(&real);
    let count = files.len();
    let listed: Vec<Value> = files.iter().take(MAX_LISTED).map(|f| json!(display(f))).collect();
    viewer.set_files(files);
    viewer.set_index((count > 0).then_some(0));
    viewer.set_rotation(0);
    Ok(json!({"folder": folder, "count": count, "truncated": count > MAX_LISTED, "files": listed, "state": state(viewer)}))
}

/// Move `delta` files in the folder (`view.previous` / `view.next`); past either end the viewer
/// stays where it is, as the arrow keys do.
pub fn step(viewer: &mut impl Viewer, delta: isize) -> Result<Value, String> {
    let Some(index) = viewer.index() else { return Err("no folder open: run `file.open_folder` (or `file.open`) first".into()) };
    let len = viewer.files().len();
    let target = index as isize + delta;
    if (0..len as isize).contains(&target) {
        viewer.set_index(Some(target as usize));
    }
    Ok(state(viewer))
}

/// Turn the viewer by `quarters` clockwise (`view.rotate_left` is 3, `view.rotate_right` is 1).
pub fn rotate(viewer: &mut impl Viewer, quarters: u32) -> Result<Value, String> {
    if viewer.index().is_none() {
        return Err("no image open: run `file.open` or `file.open_folder` first".into());
    }
    viewer.set_rotation((viewer.rotation() + quarters) % 4);
    Ok(state(viewer))
}

/// The EXIF tags the panel shows for the current image (`view.exif`).
pub fn exif(viewer: &impl Viewer) -> Result<Value, String> {
    let path = current(viewer)?;
    if !img::is_image(&path) {
        return Err(format!("`{}` is not an image: the viewer does not read video metadata", display(&path)));
    }
    let tags: Vec<Value> = img::exif_info(&path).into_iter().map(|(label, value)| json!({"label": label, "value": value})).collect();
    Ok(json!({"path": display(&path), "count": tags.len(), "tags": tags}))
}

/// The numbers the footer shows for an image (`image.info`): dimensions and file size.
pub fn info(viewer: &impl Viewer, roots: Option<&Roots>, path: Option<&str>) -> Result<Value, String> {
    let path = match path {
        Some(p) => {
            let asked = crate::img::sanitize_path_input(p);
            check(roots, &asked, Access::Read)?;
            resolve(&asked)?
        }
        None => current(viewer)?,
    };
    if !img::is_image(&path) {
        return Err(format!("`{}` is not an image: the viewer does not decode video", display(&path)));
    }
    let info = img::image_info(&path)?;
    let mut out = state(viewer);
    out["path"] = json!(display(&path));
    out["width"] = json!(info.width);
    out["height"] = json!(info.height);
    out["sizeBytes"] = json!(info.size_bytes);
    out["format"] = json!(path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase());
    Ok(out)
}

/// Re-encode the current image without EXIF (`file.export_clean`), rotated as the viewer shows it.
/// The destination defaults to `<name>.clean.png` next to the original, as the button does.
pub fn export_clean(viewer: &impl Viewer, roots: Option<&Roots>, to: Option<&str>) -> Result<Value, String> {
    let source = current(viewer)?;
    if !img::is_image(&source) {
        return Err(format!("`{}` is not an image: the viewer does not decode video", display(&source)));
    }
    check(roots, &display(&source), Access::Read)?;
    let destination = match to {
        Some(to) => {
            let asked = crate::img::sanitize_path_input(to);
            if asked.is_empty() {
                return Err("`file.export_clean`: `to` is empty".into());
            }
            PathBuf::from(asked)
        }
        None => source.with_extension("clean.png"),
    };
    check(roots, &display(&destination), Access::Write)?;
    let rotation = viewer.rotation() % 4 * 90;
    img::export(&source, &destination, rotation, None, 90)?;
    Ok(json!({"path": display(&destination), "source": display(&source), "rotation": rotation}))
}

/// The current file, or why there is none.
fn current(viewer: &impl Viewer) -> Result<PathBuf, String> {
    let files = viewer.files();
    viewer.index().and_then(|i| files.get(i)).cloned().ok_or_else(|| "no image open: run `file.open` or `file.open_folder` first".to_string())
}

/// Refuse `path` when it is outside its kind's automation roots (no roots: nothing is confined).
fn check(roots: Option<&Roots>, path: &str, access: Access) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    match roots {
        None => Ok(()),
        Some(roots) => roots.check(path, access),
    }
}

/// `path` made absolute with its links followed, without Windows' `\\?\` prefix.
fn resolve(path: &str) -> Result<PathBuf, String> {
    let real = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(PathBuf::from(img::strip_verbatim(&real.to_string_lossy())))
}

/// A path as replies and messages show it.
fn display(path: &Path) -> String {
    img::strip_verbatim(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder with two images and one unrelated file; returns it.
    fn folder(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("local-image-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.png", "B.jpg"] {
            image::RgbImage::from_pixel(4, 3, image::Rgb([(name.len() as u8) * 40, 10, 90])).save(dir.join(name)).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), b"not media").unwrap();
        dir
    }

    fn session(roots: Option<Roots>) -> Session {
        Session::new(roots)
    }

    fn exec(s: &mut Session, id: &str, params: Value) -> Result<Value, String> {
        s.exec(id, &params)
    }

    #[test]
    fn opening_a_folder_lists_its_media_and_steps_through_it() {
        let dir = folder("list");
        let mut s = session(None);
        let opened = exec(&mut s, "file.open_folder", json!({"path": dir.to_string_lossy()})).unwrap();
        assert_eq!(opened["count"], 2, "{opened}");
        assert_eq!(opened["files"][0], json!(dir.join("a.png").to_string_lossy().to_string()));
        assert_eq!(opened["state"]["position"], 1);
        let next = exec(&mut s, "view.next", json!({})).unwrap();
        assert_eq!(next["position"], 2);
        assert_eq!(next["path"], json!(dir.join("B.jpg").to_string_lossy().to_string()));
        let end = exec(&mut s, "view.next", json!({})).unwrap();
        assert_eq!(end["position"], 2, "past the last one the viewer stays");
        let back = exec(&mut s, "view.previous", json!({})).unwrap();
        assert_eq!(back["position"], 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn opening_a_file_points_the_viewer_at_its_folder() {
        let dir = folder("open");
        let mut s = session(None);
        let opened = exec(&mut s, "file.open", json!({"path": dir.join("B.jpg")})).unwrap();
        assert_eq!(opened["kind"], "image");
        assert_eq!(opened["state"]["position"], 2);
        // A folder is not a file, and a text file is not media.
        assert!(exec(&mut s, "file.open", json!({"path": dir})).unwrap_err().contains("is a folder"));
        assert!(exec(&mut s, "file.open", json!({"path": dir.join("notes.txt")})).unwrap_err().contains("not an image or a video"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn info_exif_rotation_and_export_are_the_viewers_own_numbers() {
        let dir = folder("info");
        let mut s = session(None);
        exec(&mut s, "file.open_folder", json!({"path": dir.to_string_lossy()})).unwrap();
        let info = exec(&mut s, "image.info", json!({})).unwrap();
        assert_eq!((info["width"].as_u64(), info["height"].as_u64()), (Some(4), Some(3)));
        assert_eq!(info["format"], "png");
        assert!(exec(&mut s, "image.info", json!({"path": dir.join("notes.txt")})).is_err());
        let exif = exec(&mut s, "view.exif", json!({})).unwrap();
        assert_eq!(exif["count"], 0, "a PNG has no EXIF: {exif}");
        let rotated = exec(&mut s, "view.rotate_right", json!({})).unwrap();
        assert_eq!(rotated["rotation"], 90);
        exec(&mut s, "view.rotate_left", json!({})).unwrap();
        let exported = exec(&mut s, "file.export_clean", json!({})).unwrap();
        let out = PathBuf::from(exported["path"].as_str().unwrap_or_default());
        assert_eq!(out, dir.join("a.clean.png"));
        assert!(out.is_file(), "the clean copy was written");
        // The export is rotated as the viewer shows it: a quarter turn swaps the sides.
        exec(&mut s, "view.rotate_right", json!({})).unwrap();
        let turned = exec(&mut s, "file.export_clean", json!({"to": dir.join("turned.png")})).unwrap();
        let size = image::image_dimensions(PathBuf::from(turned["path"].as_str().unwrap_or_default())).unwrap();
        assert_eq!(size, (3, 4));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_path_outside_the_roots_is_refused() {
        let inside = folder("inside");
        let outside = folder("outside");
        let roots = Roots::new(&[&inside], &[&inside]).unwrap().unwrap();
        let mut s = session(Some(roots));
        // Reading outside the read root.
        let e = exec(&mut s, "file.open_folder", json!({"path": outside.to_string_lossy()})).unwrap_err();
        assert!(e.contains("automation path rejected") && e.contains("outside the read root"), "{e}");
        // Writing outside the write root, with the image itself readable.
        let roots = Roots::new(&[&inside], &[&inside]).unwrap().unwrap();
        let mut s = session(Some(roots));
        exec(&mut s, "file.open_folder", json!({"path": inside.to_string_lossy()})).unwrap();
        let e = exec(&mut s, "file.export_clean", json!({"to": outside.join("x.png")})).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        assert!(!outside.join("x.png").exists());
        // Inside both, it works.
        exec(&mut s, "file.export_clean", json!({"to": inside.join("ok.png")})).unwrap();
        assert!(inside.join("ok.png").is_file());
        std::fs::remove_dir_all(inside).ok();
        std::fs::remove_dir_all(outside).ok();
    }

    #[test]
    fn a_kind_left_out_grants_nothing() {
        let dir = folder("kinds");
        let roots = Roots::new(&[&dir], &[]).unwrap().unwrap();
        let mut s = session(Some(roots));
        exec(&mut s, "file.open_folder", json!({"path": dir.to_string_lossy()})).unwrap();
        let e = exec(&mut s, "file.export_clean", json!({})).unwrap_err();
        assert!(e.contains("write authority is absent"), "{e}");
        assert!(!dir.join("a.clean.png").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn window_only_commands_say_where_they_run() {
        let mut s = session(None);
        for id in ["view.zoom_in", "view.fullscreen", "settings.open", "app.exit"] {
            let e = exec(&mut s, id, json!({})).unwrap_err();
            assert!(e.contains("runs in the window") && e.contains("--control"), "{id}: {e}");
        }
        assert!(exec(&mut s, "select.all", json!({})).is_err(), "an unknown id is refused");
        assert!(exec(&mut s, "view.next", json!({"n": 1})).unwrap_err().contains("unknown parameter `n`"));
    }

    #[test]
    fn every_command_runs_somewhere() {
        // A headless command must have its own arm (not the catch-all); a window-only one is
        // refused before it.
        let mut s = session(None);
        for spec in catalogue::SPECS {
            let result = exec(&mut s, spec.id, json!({}));
            if spec.app_only {
                assert!(result.as_ref().err().is_some_and(|e| e.contains("runs in the window")), "{}", spec.id);
            } else {
                assert!(!result.as_ref().err().is_some_and(|e| e.contains("no headless implementation")), "{}: {result:?}", spec.id);
            }
        }
    }

    #[test]
    fn the_control_methods_answer_command_list_and_app_info() {
        let mut s = session(None);
        let listed = s.call("command.list", &json!({"filter": "rotate"})).unwrap();
        assert_eq!(listed.as_array().map(Vec::len), Some(2));
        let info = s.call("app.info", &json!({})).unwrap();
        assert_eq!(info["id"], "local-image");
        let e = s.call("engine.execute", &json!({})).unwrap_err();
        assert!(e.contains("unknown method"), "{e}");
        // `command.execute` is how the control channel and the runs reach the commands.
        assert!(s.call("command.execute", &json!({"command": "view.next"})).is_err(), "no folder yet");
    }
}
