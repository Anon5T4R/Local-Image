//! lab-ui — esqueleto do "padrão egui" da suíte: tema (5 paletas nomeadas,
//! espelhando o padrao-apps), i18n PT/EN/ES e config JSON por app.
//! Piloto de referência; nada aqui vira padrão oficial sem decisão.

pub mod config;
pub mod i18n;
pub mod theme;
pub mod workarea;

use config::Config;
use i18n::{Key, Lang};
use theme::Appearance;

/// Um par "rótulo + controle" com o rótulo SEMPRE à esquerda no render final.
///
/// Em layout `left_to_right` isso é só a ordem natural de adição. Já num
/// cluster `right_to_left` (a toolbar do viewer), a ordem de adição é
/// ESPELHADA — adicionar rótulo antes do combo desenhava "System ▾ Theme".
/// Por isso o par inverte a ordem de adição junto com o layout, em vez de
/// deixar o espelho brigar com o rótulo.
fn pair(ui: &mut egui::Ui, label: &str, rtl: bool, control: impl FnOnce(&mut egui::Ui)) {
    if rtl {
        control(ui);
        ui.label(label);
    } else {
        ui.label(label);
        control(ui);
    }
}

/// Desenha os combos de idioma + tema (canto de configurações comum aos apps).
/// Devolve `(config_changed, forget_quick_key_clicked)`.
///
/// Rótulo ANTES do controle ("Theme [System ▾]"), como em qualquer formulário:
/// o `ComboBox::from_label` do egui desenha o rótulo DEPOIS do combo
/// ("System ▾ Theme"), então o label sai na mão e o combo entra sem ele.
/// Quem garante a posição nos dois contextos (RTL e LTR) é o `pair`.
pub fn settings_ui(ui: &mut egui::Ui, cfg: &mut Config, show_forget_quick_key: bool) -> (bool, bool) {
    let before = *cfg;
    let mut forget_clicked = false;
    // Direção do layout de QUEM chama: o `horizontal_wrapped` abaixo herda
    // dela, e é ela que decide se a ordem de adição sai espelhada.
    let rtl = ui.layout().prefer_right_to_left();

    ui.horizontal_wrapped(|ui| {
        pair(ui, i18n::t(cfg.lang, Key::Language), rtl, |ui| {
            egui::ComboBox::from_id_salt("settings-language").selected_text(cfg.lang.label()).width(80.0).show_ui(ui, |ui| {
                for l in Lang::ALL {
                    ui.selectable_value(&mut cfg.lang, l, l.label());
                }
            });
        });

        pair(ui, i18n::t(cfg.lang, Key::Theme), rtl, |ui| {
            egui::ComboBox::from_id_salt("settings-theme").selected_text(cfg.theme.label()).width(100.0).show_ui(ui, |ui| {
                for th in Appearance::ALL {
                    ui.selectable_value(&mut cfg.theme, th, th.label());
                }
            });
        });

        if show_forget_quick_key && ui.button(i18n::t(cfg.lang, Key::ForgetKey)).clicked() {
            forget_clicked = true;
        }
    });

    (*cfg != before, forget_clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todo texto desenhado no frame, com o retângulo da TINTA de cada um
    /// (`visual_bounding_rect` — o rect do galley inclui a largura justificada
    /// e, no RTL, o offset do `halign`).
    fn textos(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        out.shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), cs.shape.visual_bounding_rect())),
                _ => None,
            })
            .collect()
    }

    fn rect_of(textos: &[(String, egui::Rect)], needle: &str) -> egui::Rect {
        textos.iter().find(|(t, _)| t == needle).unwrap_or_else(|| panic!("texto {needle:?} não desenhado")).1
    }

    /// O par "label + combo" sai sempre com o RÓTULO à esquerda — inclusive no
    /// cluster `right_to_left` das toolbars, onde a ordem de adição é
    /// espelhada. Era o bug do build: "System ▾ Theme" e "English ▾ Language"
    /// (o combo ficava ANTES do rótulo nos dois apps de chamada).
    #[test]
    fn rotulo_fica_a_esquerda_do_combo_nos_dois_layouts() {
        for rtl in [false, true] {
            let ctx = egui::Context::default();
            let mut cfg = Config::default();
            let input =
                || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() };
            let out = ctx.run_ui(input(), |ui| {
                let layout = if rtl { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
                ui.with_layout(layout, |ui| {
                    settings_ui(ui, &mut cfg, false);
                });
            });

            let textos = textos(&out);
            let idioma = rect_of(&textos, i18n::t(cfg.lang, Key::Language));
            let combo_idioma = rect_of(&textos, cfg.lang.label());
            assert!(idioma.right() <= combo_idioma.left() + 0.5, "rtl={rtl}: o rótulo {idioma:?} tem que ficar à ESQUERDA do combo {combo_idioma:?}");
            let tema = rect_of(&textos, i18n::t(cfg.lang, Key::Theme));
            let combo_tema = rect_of(&textos, cfg.theme.label());
            assert!(tema.right() <= combo_tema.left() + 0.5, "rtl={rtl}: o rótulo {tema:?} tem que ficar à ESQUERDA do combo {combo_tema:?}");
            out.drop_without_applying_deltas();
        }
    }
}
