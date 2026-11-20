//! Kit de UI do Local Image — uma versão simplificada do `widgets.rs` do
//! local-pdf: modal com cartão, campo alto, pill e ghost button. As cores saem
//! do tema aplicado (`theme::apply`), então o mesmo desenho funciona no claro
//! e no escuro.

/// Recuo interno de um campo — molde do `FIELD_MARGIN` do local-pdf.
const FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);
/// Altura externa mínima de um campo de uma linha.
const FIELD_MIN_HEIGHT: f32 = 30.0;

/// Cores derivadas do tema vigente (nada fixo).
#[derive(Clone, Copy)]
pub(crate) struct Skin {
    /// Fundo do cartão do diálogo (o "sunken" da paleta).
    pub(crate) card: egui::Color32,
    /// Fundo dos campos DENTRO do diálogo (o "base", o inverso do cartão).
    field: egui::Color32,
    pub(crate) accent: egui::Color32,
    /// Texto sobre `accent` (pill primário).
    on_accent: egui::Color32,
    text: egui::Color32,
    pub(crate) muted: egui::Color32,
    pub(crate) border: egui::Color32,
    /// Lavagem de hover (ghost buttons).
    hover: egui::Color32,
}

impl Skin {
    pub(crate) fn of(ui: &egui::Ui) -> Self {
        Self::from_visuals(ui.visuals())
    }

    fn from_visuals(v: &egui::Visuals) -> Self {
        let text = v.override_text_color.unwrap_or(v.widgets.noninteractive.fg_stroke.color);
        // Accent SÓLIDO (theme.rs publica em `selection.stroke`): o
        // `selection.bg_fill` é a lavagem 35% de seleção — usá-lo aqui deixava
        // o texto dos pills primários sem contraste no tema claro.
        let accent = v.selection.stroke.color;
        Self {
            card: v.extreme_bg_color,
            field: v.panel_fill,
            accent,
            on_accent: if luminance(accent) > 150.0 { egui::Color32::BLACK } else { egui::Color32::WHITE },
            text,
            muted: text.gamma_multiply(0.6),
            border: if v.dark_mode { egui::Color32::from_white_alpha(30) } else { egui::Color32::from_black_alpha(30) },
            hover: if v.dark_mode { egui::Color32::from_white_alpha(18) } else { egui::Color32::from_black_alpha(14) },
        }
    }
}

/// Luminância perceptual (0–255) — decide texto claro ou escuro sobre o accent.
fn luminance(c: egui::Color32) -> f32 {
    0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
}

/// Campo de uma linha com altura de clique decente.
pub(crate) fn line(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(FIELD_MARGIN).min_size(egui::vec2(0.0, FIELD_MIN_HEIGHT))
}

/// Moldura do diálogo: cartão com canto de 10, borda sutil e a sombra do tema.
pub(crate) fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    let style = ctx.style_of(ctx.theme());
    let skin = Skin::from_visuals(&style.visuals);
    egui::Frame::NONE
        .fill(skin.card)
        .stroke(egui::Stroke::new(1.0_f32, skin.border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(20, 16))
        .shadow(style.visuals.window_shadow)
}

/// Visuals de dentro do diálogo: campos com contorno e no tom "base" (o cartão
/// é o "sunken" — sem isso o campo some no fundo do modal).
pub(crate) fn begin_dialog(ui: &mut egui::Ui) {
    let skin = Skin::of(ui);
    let v = ui.visuals_mut();
    v.extreme_bg_color = skin.field;
    v.selection.stroke = egui::Stroke::new(1.5_f32, skin.accent);
    let w = &mut v.widgets;
    w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, skin.border);
    w.inactive.weak_bg_fill = skin.field;
    w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, skin.muted);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = egui::CornerRadius::same(4);
    }
    ui.spacing_mut().interact_size.y = FIELD_MIN_HEIGHT;
}

/// Botão pill: `primary` preenche com o accent, o secundário fica no contorno.
/// Desenhado à mão porque o `egui::Button` não tem raio/altura própria.
pub(crate) fn pill(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let skin = Skin::of(ui);
    if primary {
        pill_colored(ui, label, skin.accent, skin.on_accent, false)
    } else {
        pill_colored(ui, label, egui::Color32::TRANSPARENT, skin.text, true)
    }
}

/// Pill com cores explícitas; `outline` desenha só a borda.
fn pill_colored(ui: &mut egui::Ui, label: &str, fill: egui::Color32, ink: egui::Color32, outline: bool) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let ink = if enabled { ink } else { skin.muted };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 28.0, 28.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let hovered = resp.hovered() && enabled;
    let fill = if !enabled {
        fill.gamma_multiply(0.45)
    } else if hovered {
        fill.gamma_multiply(1.12)
    } else {
        fill.gamma_multiply(0.9)
    };
    let fill = if outline { if hovered { skin.hover } else { egui::Color32::TRANSPARENT } } else { fill };
    let stroke = if outline { egui::Stroke::new(1.0_f32, skin.border) } else { egui::Stroke::NONE };
    ui.painter().rect(rect, egui::CornerRadius::same(14), fill, stroke, egui::StrokeKind::Inside);
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

/// Ação de linha: transparente até o hover (o "ghost button" do local-pdf).
pub(crate) fn ghost(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ghost_ink(ui, label, Skin::of(ui).muted)
}

/// [`ghost`] com tinta própria (o ★ favorito usa o accent quando ativo).
fn ghost_ink(ui: &mut egui::Ui, label: &str, ink: egui::Color32) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    // Desabilitado: tinta apagada. O egui não reporta hover em widget
    // desabilitado, então o ghost não "acende" sozinho — mas sem o apagado
    // ele ficaria igual ao habilitado.
    let ink = if enabled { ink } else { ink.gamma_multiply(0.45) };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contraste WCAG entre duas cores opacas.
    fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
        let lum = |c: egui::Color32| {
            let lin = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (hi, lo) = (lum(a).max(lum(b)), lum(a).min(lum(b)));
        (hi + 0.05) / (lo + 0.05)
    }

    /// O texto do pill primário (accent cheio) e os links passam de 4,5:1 nos dois temas.
    #[test]
    fn the_primary_pill_and_links_are_legible_in_both_themes() {
        for tokens in [crate::theme::Tokens::light(), crate::theme::Tokens::dark()] {
            let skin = Skin::from_visuals(&crate::theme::visuals(&tokens));
            assert_eq!(skin.accent, tokens.accent, "o pill usa o accent cheio, não a seleção translúcida");
            let pill = contrast(skin.on_accent, skin.accent);
            let link = contrast(skin.accent, tokens.base);
            assert!(pill >= 4.5, "texto do pill: {pill:.2}:1 (dark={})", tokens.dark);
            assert!(link >= 4.5, "link sobre a base: {link:.2}:1 (dark={})", tokens.dark);
        }
    }
}
