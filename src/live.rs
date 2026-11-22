//! The window's side of the control channel (`local-image --control PORT`): the same commands the
//! headless session runs, executed with the app's own state, so an agent's `view.next` moves the
//! viewer the user is looking at.
//!
//! Each frame [`ImageApp::pump_control`] answers what arrived. The commands themselves are the
//! shared ones (`automation::session`), run over the app's fields; here live only the window's
//! effects afterwards — the folder shown, the image decoded, the EXIF panel.

use std::path::PathBuf;

use craft_mcp::automation::Roots;
use serde_json::{Value, json};

use crate::automation::catalogue;
use crate::automation::session::{self, Viewer};
use crate::{ImageApp, img};

/// The app's viewer fields, borrowed apart from the rest of it: a command runs on them while the
/// automation roots and the window are still reachable.
struct Window<'a> {
    files: &'a mut Vec<PathBuf>,
    index: &'a mut Option<usize>,
    rotation: &'a mut u32,
}

impl Viewer for Window<'_> {
    fn files(&self) -> &[PathBuf] {
        self.files
    }
    fn index(&self) -> Option<usize> {
        *self.index
    }
    fn set_files(&mut self, files: Vec<PathBuf>) {
        *self.files = files;
    }
    fn set_index(&mut self, index: Option<usize>) {
        *self.index = index;
    }
    fn rotation(&self) -> u32 {
        *self.rotation
    }
    fn set_rotation(&mut self, quarters: u32) {
        *self.rotation = quarters % 4;
    }
}

impl ImageApp {
    /// Answer the control channel's pending requests with this frame's state, if the app was
    /// started with `--control`.
    pub(crate) fn pump_control(&mut self, ctx: &egui::Context) {
        // Taken out so the dispatch can borrow the whole app; the channel goes back afterwards.
        let Some(mut control) = self.control.take() else { return };
        control.pump(|method, params| self.live_call(ctx, method, params));
        self.control = Some(control);
        // A request may have arrived while the window was idle: keep answering while the channel
        // is open (it exists only in the `--control` mode).
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    /// One control-channel method of the running app.
    fn live_call(&mut self, ctx: &egui::Context, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "command.execute" => {
                let id = params.get("command").and_then(Value::as_str).ok_or("command.execute: missing string `command`")?;
                let args = params.get("params").cloned().unwrap_or(Value::Null);
                self.live_exec(ctx, id, &args)
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
                "backend": "window",
                "state": self.with_window(|w, _| session::state(w)),
            })),
            other => Err(format!("unknown method `{other}`: this app answers command.execute, command.list and app.info")),
        }
    }

    /// Run one command with the app's state, then apply the window's effects.
    fn live_exec(&mut self, ctx: &egui::Context, id: &str, params: &Value) -> Result<Value, String> {
        if !catalogue::has(id) {
            return Err(format!("unknown command `{id}`: command_list lists what this app has"));
        }
        let args = catalogue::arguments(id, params)?;
        let text =
            |key: &str| -> Result<&str, String> { args.get(key).and_then(Value::as_str).ok_or_else(|| format!("`{id}`: `{key}` must be a string")) };
        match id {
            "file.open" => {
                let path = text("path")?;
                let out = self.with_window(|w, roots| session::open_file(w, roots, path))?;
                self.show_current();
                Ok(out)
            }
            "file.open_folder" => {
                let path = text("path")?;
                let out = self.with_window(|w, roots| session::open_folder(w, roots, path))?;
                self.show_current();
                Ok(out)
            }
            "file.export_clean" => self.with_window(|w, roots| session::export_clean(w, roots, args.get("to").and_then(Value::as_str))),
            "image.info" => self.with_window(|w, roots| session::info(w, roots, args.get("path").and_then(Value::as_str))),
            "view.previous" | "view.next" => {
                let delta = if id == "view.next" { 1 } else { -1 };
                self.with_window(|w, _| session::step(w, delta))?;
                self.show_current();
                Ok(self.with_window(|w, _| session::state(w)))
            }
            "view.rotate_left" | "view.rotate_right" => {
                let quarters = if id == "view.rotate_right" { 1 } else { 3 };
                let out = self.with_window(|w, _| session::rotate(w, quarters))?;
                self.refit();
                Ok(out)
            }
            "view.exif" => {
                // The panel's data, and the panel itself toggles as the menu item does.
                let out = self.with_window(|w, _| session::exif(w))?;
                self.show_exif = !self.show_exif;
                Ok(out)
            }
            _ => {
                // What is left needs the window: the viewport, the dialogs, the updater, exit.
                // The palette's own dispatch, so the behaviour cannot drift from the user's.
                self.run_command(ctx, id);
                Ok(self.with_window(|w, _| session::state(w)))
            }
        }
    }

    /// Run `f` on the app's viewer fields with the automation roots in hand.
    fn with_window<R>(&mut self, f: impl FnOnce(&mut Window<'_>, Option<&Roots>) -> R) -> R {
        let mut window = Window { files: &mut self.files, index: &mut self.idx, rotation: &mut self.rot };
        f(&mut window, self.roots.as_ref())
    }

    /// Show the folder's current file: the footer's folder and a fresh decode (as `goto` does).
    fn show_current(&mut self) {
        self.dir =
            self.files.first().and_then(|f| f.parent()).map(img::strip_verbatim_path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(index) = self.idx {
            self.goto(index);
        } else {
            self.tex = None;
            self.exif = None;
        }
    }
}
