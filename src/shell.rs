//! A casca da suíte no Image: os botões da barra de cima (engrenagem, tema, lupa), a janela de Configurações
//! (idioma, tema e Sobre) e a paleta de comandos. Os comandos rodam o mesmo código dos atalhos do viewer.

use craft_icons::{Icon, ThemeMode};
use craft_palette::Command;
use craft_update::updater::Status;
use egui::{Key, KeyboardShortcut, Modifiers};
use lab_ui::i18n::{self, Key as Text, Lang};
use lab_ui::theme::{self, Appearance};

use crate::ui::{begin_dialog, modal_frame};
use crate::ImageApp;

/// O modo da suíte que corresponde à preferência salva (os ids são os mesmos: "system", "light", "dark").
pub(crate) fn mode_of(appearance: Appearance) -> ThemeMode {
    ThemeMode::from_id(appearance.code())
}

/// A preferência salva de um modo.
pub(crate) fn appearance_of(mode: ThemeMode) -> Appearance {
    Appearance::from_code(mode.id()).unwrap_or_default()
}

fn shortcut_text(ctx: &egui::Context) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
}

impl ImageApp {
    /// Grava o config.json (idioma e tema).
    fn save_config(&self) {
        let _ = lab_ui::config::save_to(&self.config_file, &self.cfg);
    }

    /// Troca o modo de tema: aplica e grava.
    pub(crate) fn set_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.cfg.theme = appearance_of(mode);
        theme::apply(ctx, self.cfg.theme);
        self.save_config();
    }

    fn set_lang(&mut self, lang: Lang) {
        self.cfg.lang = lang;
        self.save_config();
    }

    /// Os controles da suíte à direita da barra de cima, num layout da direita para a esquerda: engrenagem
    /// (mais à direita), tema e lupa.
    pub(crate) fn shell_buttons(&mut self, ui: &mut egui::Ui) {
        let lang = self.cfg.lang;
        // Longe da borda da janela, e um dedo de distância entre os botões.
        ui.add_space(8.0);
        if craft_icons::ui::button(ui, Icon::Settings, i18n::tr(lang, "Settings")).clicked() {
            self.settings_open = true;
        }
        ui.add_space(6.0);
        let mut mode = mode_of(self.cfg.theme);
        let tooltip = i18n::tr_fmt(lang, "Theme: {mode}", &[("mode", i18n::tr(lang, mode.label()))]);
        if craft_icons::ui::theme_button(ui, &mut mode, &tooltip) {
            self.set_mode(ui.ctx(), mode);
        }
        ui.add_space(6.0);
        let search = i18n::tr_fmt(lang, "Search commands ({shortcut})", &[("shortcut", &shortcut_text(ui.ctx()))]);
        if craft_icons::ui::button(ui, Icon::Search, &search).clicked() {
            self.palette.open();
        }
    }

    /// Configurações: idioma, tema e a seção Sobre (versão, repositório, atualizações).
    pub(crate) fn settings_modal(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let lang = self.cfg.lang;
        let t = |source: &'static str| i18n::tr(lang, source);
        let mut close = false;
        let mut check = false;
        let mut new_lang = lang;
        let mut mode = mode_of(self.cfg.theme);
        let status = self.updater.status().clone();
        let resp = egui::Modal::new(egui::Id::unique("settings")).frame(modal_frame(ctx)).show(ctx, |ui| {
            begin_dialog(ui);
            // Cabe na janela estreita: a moldura do diálogo come 40 px e a borda 16.
            ui.set_width(320.0_f32.min(ctx.viewport_rect().width() - 56.0));
            ui.horizontal(|ui| {
                ui.heading(t("Settings"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if craft_icons::ui::close_button(ui, i18n::t(lang, Text::Close)).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            egui::Grid::new("settings-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(i18n::t(lang, Text::Language));
                egui::ComboBox::from_id_salt("settings-lang").selected_text(lang.label()).show_ui(ui, |ui| {
                    for choice in Lang::ALL {
                        ui.selectable_value(&mut new_lang, choice, choice.label());
                    }
                });
                ui.end_row();
                ui.label(i18n::t(lang, Text::Theme));
                ui.horizontal(|ui| {
                    for choice in ThemeMode::EVERY {
                        ui.selectable_value(&mut mode, choice, t(choice.label()));
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.strong(t("About Local Image"));
            ui.label(i18n::tr_fmt(lang, "version {version}", &[("version", env!("CARGO_PKG_VERSION"))]));
            ui.hyperlink_to("github.com/Anon5T4R/Local-Image", env!("CARGO_PKG_REPOSITORY"));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let checking = matches!(status, Status::Checking);
                if ui.add_enabled(!checking, egui::Button::new(t("Check for Updates…"))).clicked() {
                    check = true;
                }
                match &status {
                    Status::Checking => {
                        ui.spinner();
                    }
                    Status::UpToDate => {
                        ui.label(t("You are on the newest version."));
                    }
                    Status::Failed(reason) => {
                        ui.label(format!("{}: {reason}", i18n::t(lang, Text::Error)));
                    }
                    Status::Idle => {}
                }
            });
        });
        if new_lang != lang {
            self.set_lang(new_lang);
        }
        if mode_of(self.cfg.theme) != mode {
            self.set_mode(ctx, mode);
        }
        if check {
            self.updater.check_now(ctx);
        }
        if close || resp.should_close() {
            self.settings_open = false;
            self.updater.dismiss_status();
        }
    }

    /// O que a paleta lista agora: os botões e atalhos do viewer, com o mesmo "ligado" deles.
    pub(crate) fn commands(&self) -> Vec<Command> {
        let lang = self.cfg.lang;
        let tr = |source: &'static str| i18n::tr(lang, source);
        let (file, view, settings, help, app) = (tr("File"), tr("View"), tr("Settings"), tr("Help"), "Local Image");
        let has_image = self.tex.is_some();
        let mut list = vec![
            Command::new("file.open_folder", tr("Open folder…"), file),
            Command::new("file.export_clean", i18n::t(lang, Text::ExportClean), file).enabled(has_image),
            Command::new("view.previous", tr("Previous image"), view).shortcut("←").enabled(self.idx.is_some_and(|i| i > 0)),
            Command::new("view.next", tr("Next image"), view).shortcut("→").enabled(self.idx.is_some_and(|i| i + 1 < self.files.len())),
            Command::new("view.zoom_in", tr("Zoom in"), view).enabled(has_image),
            Command::new("view.zoom_out", tr("Zoom out"), view).enabled(has_image),
            Command::new("view.fit", tr("Fit to window"), view).shortcut("0").enabled(has_image),
            Command::new("view.center", tr("Center image"), view).shortcut("C").enabled(has_image),
            Command::new("view.rotate_left", tr("Rotate left"), view).shortcut("[").enabled(has_image),
            Command::new("view.rotate_right", tr("Rotate right"), view).shortcut("]").enabled(has_image),
            Command::new("view.fullscreen", tr("Full screen"), view).shortcut("F"),
            Command::new("view.resize_window", tr("Resize window to image"), view).shortcut("R").enabled(has_image),
            Command::new("view.exif", tr("EXIF panel"), view).shortcut("I"),
            Command::new("settings.open", tr("Settings"), settings),
        ];
        for mode in ThemeMode::EVERY {
            let label = i18n::tr_fmt(lang, "Theme: {mode}", &[("mode", i18n::tr(lang, mode.label()))]);
            list.push(Command::new(format!("settings.theme.{}", mode.id()), label, settings));
        }
        let checking = matches!(self.updater.status(), Status::Checking);
        list.extend([
            Command::new("help.updates", tr("Check for Updates…"), help).enabled(!checking),
            Command::new("help.about", tr("About Local Image"), help),
            Command::new("app.exit", tr("Exit"), app),
        ]);
        list
    }

    /// Roda o que a paleta devolveu, pelo mesmo código dos botões e atalhos.
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, id: &str) {
        match id {
            "file.open_folder" => {
                self.pickers.ask_folder(ctx, Some(&self.dir));
            }
            "file.export_clean" => self.export_clean(),
            "view.previous" => self.step(-1),
            "view.next" => self.step(1),
            "view.zoom_in" => self.zoom_request = Some(1.25),
            "view.zoom_out" => self.zoom_request = Some(0.8),
            "view.fit" => self.refit(),
            "view.center" => self.center_pending = true,
            "view.rotate_left" => self.rotate(3),
            "view.rotate_right" => self.rotate(1),
            "view.fullscreen" => self.toggle_fullscreen(ctx),
            "view.resize_window" => self.resize_to_image(),
            "view.exif" => self.show_exif = !self.show_exif,
            "settings.open" | "help.about" => self.settings_open = true,
            "help.updates" => {
                // O resultado aparece nas Configurações, como no botão de lá.
                self.settings_open = true;
                self.updater.check_now(ctx);
            }
            "app.exit" => self.quit = true,
            _ => {
                if let Some(mode) = id.strip_prefix("settings.theme.") {
                    self.set_mode(ctx, ThemeMode::from_id(mode));
                }
            }
        }
        // Zoom e redimensionamento só acontecem no painel central e no próximo quadro.
        ctx.request_repaint();
    }

    /// A paleta, quando aberta.
    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette.is_open() {
            return;
        }
        let lang = self.cfg.lang;
        let commands = self.commands();
        let labels = craft_palette::Labels { placeholder: i18n::tr(lang, "Search commands"), empty: i18n::tr(lang, "No command matches.") };
        if let Some(id) = self.palette.show(ctx, &labels, &commands) {
            self.run_command(ctx, &id);
        }
    }
}
