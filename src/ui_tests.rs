//! Testes de UI sem janela (egui_kittest) da casca da suíte: o botão de tema, a engrenagem, a lupa, as
//! Configurações e a paleta. Nenhum teste toca o config.json real nem a rede.

use std::path::{Path, PathBuf};

use craft_icons::ThemeMode;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::ImageApp;
use crate::prefs::Prefs;

/// A largura mínima que as capturas de revisão e o teste de barra estreita usam (o app não fixa mínimo:
/// "Abrir com" numa imagem pequena abre uma janela pequena).
const NARROW: f32 = 340.0;

/// Pasta temporária própria do teste (o config.json de teste vai para lá).
fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("local-image-ui-{name}-{nanos}"))
}

fn app(name: &str, prefs: Prefs) -> ImageApp {
    let mut app = ImageApp::new(prefs, None, false);
    app.config_file = scratch(name).join("config.json");
    app
}

/// Um app com três "fotos" na lista (a segunda aberta) e uma textura própria: a barra mostra o contador, o
/// girar, o exportar e o centralizar, e os comandos de imagem da paleta ficam ligados.
fn app_with_image(name: &str, prefs: Prefs) -> ImageApp {
    let mut app = app(name, prefs);
    app.files = ["a.png", "b.png", "c.png"].iter().map(PathBuf::from).collect();
    app.idx = Some(1);
    app
}

/// Textura de teste: um degradê 800x500 (sem decodificar arquivo).
fn test_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let (w, h) = (800usize, 500usize);
    let pixels = (0..w * h).map(|i| egui::Color32::from_rgb((i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 160)).collect();
    ctx.load_texture("view", egui::ColorImage::new([w, h], pixels), egui::TextureOptions::default())
}

// O kittest não tem o provedor de fontes do sistema: ⟳, ⊙ e ← viram "tofu" nos testes. A checagem de
// acessibilidade fica desligada: os botões só-ícone da suíte ganham nome pela dica, e a do kittest não a vê.
fn harness_sized(app: ImageApp, width: f32, with_image: bool) -> Harness<'static, ImageApp> {
    Harness::builder()
        .with_size(egui::vec2(width, 640.0))
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, app: &mut ImageApp| {
                egui_extras::install_image_loaders(ui.ctx());
                // O idioma vale por thread (a interface roda numa só): cada teste fixa o seu a cada quadro.
                crate::i18n::set_language(&app.prefs.language);
                crate::theme::apply(ui.ctx(), app.prefs.mode());
                if with_image && app.tex.is_none() {
                    let name = app.files.get(app.idx.unwrap_or(0)).cloned().unwrap_or_default();
                    app.tex = Some((name, test_texture(ui.ctx())));
                }
                app.frame(ui);
            },
            app,
        )
}

fn harness(app: ImageApp) -> Harness<'static, ImageApp> {
    harness_sized(app, 700.0, false)
}

/// Os botões da barra de cima, da esquerda para a direita (os de texto são largos: ficam de fora).
fn bar_buttons(harness: &Harness<'_, ImageApp>) -> Vec<egui::Rect> {
    let mut rects: Vec<egui::Rect> =
        harness.get_all_by_role(Role::Button).map(|node| node.rect()).filter(|rect| rect.min.y < 40.0 && rect.width() < 40.0).collect();
    rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    rects
}

fn click_at(harness: &mut Harness<'_, ImageApp>, pos: egui::Pos2) {
    let push = |harness: &mut Harness<'_, ImageApp>, event: egui::Event| harness.input_mut().events.push(event);
    push(harness, egui::Event::PointerMoved(pos));
    harness.run_steps(1);
    let button = egui::PointerButton::Primary;
    push(harness, egui::Event::PointerButton { pos, button, pressed: true, modifiers: egui::Modifiers::default() });
    harness.run_steps(1);
    push(harness, egui::Event::PointerButton { pos, button, pressed: false, modifiers: egui::Modifiers::default() });
    harness.run_steps(2);
}

/// Clica num dos três botões da suíte, contando da DIREITA: 0 = engrenagem, 1 = tema, 2 = lupa.
fn click_bar(harness: &mut Harness<'_, ImageApp>, from_right: usize) {
    let rects = bar_buttons(harness);
    assert!(rects.len() > from_right, "a barra não tem o botão {from_right}: {rects:?}");
    click_at(harness, rects[rects.len() - 1 - from_right].center());
}

const GEAR: usize = 0;
const THEME: usize = 1;
const SEARCH: usize = 2;

fn press(harness: &mut Harness<'_, ImageApp>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    harness.run_steps(2);
}

fn shot(harness: &mut Harness<'_, ImageApp>, name: &str) {
    if std::env::var_os("LOCALIMAGE_QA_RENDER").is_none() {
        return;
    }
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-casca");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(image) = harness.render() {
        let _ = image.save(directory.join(format!("{name}.png")));
    }
}

/// O botão de tema da barra gira Automático -> Claro -> Escuro -> Automático e grava a escolha.
#[test]
fn the_theme_button_cycles_the_three_modes() {
    let mut harness = harness(app("theme", Prefs::default()));
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Light);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Dark);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    // Cada troca foi gravada: o arquivo de teste (nunca o real) guarda o modo atual.
    let saved = Prefs::load_from(&harness.state().config_file);
    assert_eq!(saved.mode(), ThemeMode::System);
}

/// Uma preferência de tema gravada por versões antigas (id que a casca não conhece) abre no Automático.
#[test]
fn an_old_theme_id_falls_back_to_automatic() {
    let dir = scratch("old-theme");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("config.json");
    std::fs::write(&file, r#"{"theme":"PunkPrincess","lang":"En"}"#).expect("write config");
    let prefs = Prefs::load_from(&file);
    assert_eq!(prefs.mode(), ThemeMode::System);
    assert_eq!(prefs.language, "en");
}

/// A engrenagem abre as Configurações; lá o idioma troca e o tema vira pelas três opções.
#[test]
fn the_gear_opens_settings_with_language_and_theme() {
    let mut harness = harness(app("gear", Prefs::new("en", ThemeMode::Dark)));
    click_bar(&mut harness, GEAR);
    assert!(harness.state().settings_open);
    harness.get_by_label("About Local Image");
    shot(&mut harness, "settings-dark");
    harness.get_by_label("Light").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::Light);
    harness.get_by_label("Automatic").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.mode(), ThemeMode::System);
    // O idioma sai da barra e mora aqui: o combo troca para o português.
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(2);
    harness.get_by_label("Português (Brasil)").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.language, "pt-br");
    harness.get_by_label("Sobre o Local Image");
    // Esc fecha a janela.
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().settings_open);
}

/// A lupa e o Ctrl+K abrem a paleta; Enter roda o comando escolhido.
#[test]
fn the_search_icon_and_ctrl_k_open_the_palette_and_run_a_command() {
    let mut harness = harness(app("palette", Prefs::new("en", ThemeMode::Light)));
    assert!(!harness.state().palette.is_open());
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().palette.is_open());

    press(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    assert!(harness.state().palette.is_open());
    harness.input_mut().events.push(egui::Event::Text("about".into()));
    harness.run_steps(2);
    press(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(!harness.state().palette.is_open());
    assert!(harness.state().settings_open, "About abre as Configurações");
}

/// Uma imagem aberta liga os comandos de imagem da paleta, e um comando gira o viewer como o atalho.
#[test]
fn a_palette_command_rotates_the_open_image() {
    let mut harness = harness_sized(app_with_image("rotate", Prefs::new("en", ThemeMode::Dark)), 700.0, true);
    harness.run_steps(2);
    let enabled: Vec<(String, bool)> = harness.state().commands().into_iter().map(|c| (c.id, c.enabled)).collect();
    for id in ["view.previous", "view.next", "view.zoom_in", "view.rotate_right", "file.export_clean"] {
        assert!(enabled.iter().any(|(i, on)| i == id && *on), "{id} deveria estar ligado");
    }
    press(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text("rotate right".into()));
    harness.run_steps(2);
    press(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(harness.state().rot, 1);
    // Zoom da paleta sai do encaixe e amplia.
    assert!(harness.state().fit);
    press(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text("zoom in".into()));
    harness.run_steps(2);
    press(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    harness.run_steps(2);
    assert!(!harness.state().fit);
    let first = harness.state().zoom;
    press(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text("zoom in".into()));
    harness.run_steps(2);
    press(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    harness.run_steps(2);
    assert!(harness.state().zoom > first * 1.2, "o segundo zoom amplia de novo");
}

/// Sem imagem aberta, só os comandos que não dependem dela ficam ligados.
#[test]
fn image_commands_are_off_without_an_image() {
    let app = app("empty", Prefs::default());
    for command in app.commands() {
        let needs_image = matches!(
            command.id.as_str(),
            "file.export_clean"
                | "view.previous"
                | "view.next"
                | "view.zoom_in"
                | "view.zoom_out"
                | "view.fit"
                | "view.center"
                | "view.rotate_left"
                | "view.rotate_right"
                | "view.resize_window"
        );
        assert_eq!(command.enabled, !needs_image || command.id == "view.fullscreen", "{}", command.id);
    }
}

/// "Exit" na paleta pede o encerramento (o quadro responde com Close).
#[test]
fn exit_in_the_palette_asks_to_quit() {
    let mut harness = harness(app("exit", Prefs::new("en", ThemeMode::Dark)));
    press(&mut harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text("exit".into()));
    harness.run_steps(2);
    press(&mut harness, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(harness.state().quit);
}

/// Com a paleta aberta, as setas e as letras do viewer não navegam nem giram (a paleta é dona delas).
#[test]
fn viewer_shortcuts_stay_quiet_while_the_palette_is_open() {
    let mut harness = harness_sized(app_with_image("quiet", Prefs::new("en", ThemeMode::Light)), 700.0, true);
    harness.run_steps(2);
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::ArrowRight, egui::Modifiers::NONE);
    press(&mut harness, egui::Key::CloseBracket, egui::Modifiers::NONE);
    assert_eq!(harness.state().idx, Some(1));
    assert_eq!(harness.state().rot, 0);
}

/// Os botões da barra continuam inteiros e fora do título na largura estreita, com a imagem aberta.
#[test]
fn the_top_bar_fits_a_narrow_window() {
    let mut harness = harness_sized(app_with_image("narrow", Prefs::new("pt-br", ThemeMode::Light)), NARROW, true);
    harness.run_steps(2);
    let title = harness.get_by_label("Local Image").rect();
    let rects = bar_buttons(&harness);
    assert_eq!(rects.len(), 3, "na janela mínima a barra fica com lupa, tema e engrenagem: {rects:?}");
    for rect in &rects {
        assert!(rect.max.x <= NARROW && rect.min.x >= title.max.x, "botão fora da janela ou sobre o título {title:?}: {rect:?}");
    }
    shot(&mut harness, "narrow-light");
}

/// Na largura média (entre a mínima e a larga) nada da barra se sobrepõe: título, ferramentas e botões da suíte.
#[test]
fn the_top_bar_does_not_overlap_at_medium_widths() {
    for width in [400.0_f32, 439.0, 440.0, 479.0, 480.0, 600.0] {
        let mut harness = harness_sized(app_with_image("medium", Prefs::new("pt-br", ThemeMode::Light)), width, true);
        harness.run_steps(2);
        let mut rects = bar_buttons(&harness);
        // O "Exportar limpa" (largo) entra na conta: está na faixa da barra e não é só-ícone.
        rects.extend(harness.get_all_by_role(Role::Button).map(|n| n.rect()).filter(|r| r.min.y < 40.0 && r.width() >= 40.0));
        rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
        let title = harness.get_by_label("Local Image").rect();
        assert!(rects.first().is_some_and(|r| r.min.x >= title.max.x), "{width}: botão sobre o título {title:?}: {rects:?}");
        for pair in rects.windows(2) {
            assert!(pair[0].max.x <= pair[1].min.x + 0.5, "{width}: botões se sobrepõem: {pair:?}");
        }
        assert!(rects.last().is_some_and(|r| r.max.x <= width), "{width}: botão fora da janela: {rects:?}");
    }
}

/// As larguras das capturas de revisão (estreita e média), com a imagem aberta, nos dois temas.
#[test]
fn review_shots() {
    for (width, label) in [(NARROW, "340"), (1100.0, "1100")] {
        for (mode, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            let mut harness = harness_sized(app_with_image("shots", Prefs::new("en", mode)), width, true);
            harness.run_steps(2);
            shot(&mut harness, &format!("bar-{name}-{label}"));
            click_bar(&mut harness, GEAR);
            shot(&mut harness, &format!("settings-{name}-{label}"));
            press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
            click_bar(&mut harness, SEARCH);
            shot(&mut harness, &format!("palette-{name}-{label}"));
        }
    }
}
