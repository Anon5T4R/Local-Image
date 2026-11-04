//! Local Image — visualizador de imagens 100% local e offline.
//! Viewer + EXIF + export re-encodado (privacidade: EXIF nunca sobrevive).
//! Núcleo (`img.rs`) portado do LocalImage oficial; promovido do lab-image
//! (egui-lab) pra repo standalone da suíte Local.
//!
//! Visualização: a image crate decodifica → textura egui; zoom por roda do
//! mouse com âncora no cursor, pan com arrastar, encaixe na janela (tecla
//! 0), centralizar mantendo o zoom (tecla C), tela cheia preenchendo a tela
//! (F), janela no tamanho da imagem (R), setas ←/→ navegam a pasta, i
//! abre/fecha o painel EXIF. Vídeo não é decodificado: vai pro player do
//! sistema e o viewer segue pra próxima imagem à frente.
//!
//! Boot SEM arquivo = estado vazio: o miolo mostra o hint de abrir pasta, com
//! a pill primária que chama o seletor NATIVO do sistema (rfd) logo acima, e
//! o rodapé aceita o caminho digitado/colado. O app é só o VIEWER: gerenciar
//! arquivos (renomear, mover, excluir, criar pasta) é do gerenciador do SO,
//! que faz isso melhor — aqui o app fica leve e simples.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod img;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use lab_ui::workarea::fit_to_work_area;
use ui::{begin_dialog, ghost, line, modal_frame, pill};

const APP_ID: &str = "local-image";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// O seletor NATIVO de pasta existe neste build? O `rfd` é dependência só do
/// Windows (fora dele o crate nem compila sem gtk/xdg-portal — ver
/// `Cargo.toml`); no Linux o caminho continua sendo digitado no rodapé.
const NATIVE_PICKER: bool = cfg!(windows);

/// Ícone da janela (além do .exe cunhado no build.rs — Windows pega os
/// dois; AppImage/desktop só vendo com o PNG do pacote).
const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Decodifica o PNG do ícone pra `IconData` (o `.with_icon` do egui).
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = img.to_rgba8().into_raw();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba })
}

fn main() -> eframe::Result<()> {
    // JPEG XL: registra o decoder jxl-rs (o mesmo do Chrome/Firefox) no
    // `image` — processo inteiro, ANTES de qualquer open/dimensions.
    jxl_image_rs_integration::register_image_decoding_hook();

    // Windows: o helper de update é uma CÓPIA deste programa iniciada com
    // `--finish-update` (craft_update). Roda ANTES de tudo.
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::LOCALIMAGE, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }

    let cfg = config::load(APP_ID);

    // "Abrir com" do Windows manda o caminho como primeiro arg.
    // canonicalize normaliza o casing/curto-caminho (o match com a listagem
    // depende disso); strip_verbatim tira o `\\?\` que o canonicalize devolve —
    // sem ele o prefixo vaza no rodapé e no breadcrumb.
    let initial_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .and_then(|p| p.canonicalize().ok())
        .map(|p| PathBuf::from(img::strip_verbatim(&p.to_string_lossy())));

    // "Abrir com" = janela no tamanho REAL da imagem (header only — sem
    // decode). ppp ainda é desconhecido aqui (points ≈ px em 100%); o
    // primeiro update corrige a escala e clampa na tela.
    let inner_size = initial_file.as_ref().and_then(|p| image::image_dimensions(p).ok()).map(|(w, h)| [w as f32, h as f32]).unwrap_or([900.0, 640.0]);

    let mut viewport = egui::ViewportBuilder::default().with_title("Local Image").with_inner_size(inner_size);
    // Ícone da janela (o .exe já cunha o mesmo no build.rs — Resources).
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            let opened_with_file = initial_file.is_some();
            let mut app = ImageApp::new(cfg, initial_file, opened_with_file);
            // Check de update no arranque: achou release nova, o prompt do
            // updater abre sozinho (craft_update::dialog).
            app.updater.check_at_start(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// Abre `target` (URL ou caminho) no app padrão do sistema — o "abrir com"
/// do SO. É a base única do `open_url` (Download do prompt do updater) e do
/// `open_path` (vídeo no player da máquina).
fn shell_open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb = HSTRING::from("open");
        let file = HSTRING::from(target);
        // > 32 = lançou.
        let res = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if res.0 as usize <= 32 {
            return Err(format!("ShellExecute ao abrir {target}: código {}", res.0 as usize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(target).spawn().map(|_| ()).map_err(|e| format!("xdg-open {target}: {e}"))
    }
}

/// Abre a página da release no navegador — ação "Download" do prompt do
/// updater, para instalações que não se auto-atualizam (zip solto, dev build).
fn open_url(url: &str) {
    if let Err(e) = shell_open(url) {
        eprintln!("update: {e}");
    }
}

/// Abre um arquivo no app padrão do sistema — vídeo vai pro player da
/// máquina (a image crate não decodifica contêiner).
fn open_path(path: &Path) -> Result<(), String> {
    shell_open(&path.to_string_lossy())
}

/// Abre o seletor NATIVO de pasta (rfd), começando na pasta atual quando há
/// uma. Síncrono: o modal do SO bloqueia o frame enquanto está aberto — é o
/// comportamento esperado de um diálogo nativo. `None` = usuário cancelou (ou
/// build sem o rfd: no Linux o caminho é digitado) — silêncio, sem status.
fn pick_folder(current: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut dialog = rfd::FileDialog::new();
        if !current.is_empty() {
            dialog = dialog.set_directory(current);
        }
        dialog.pick_folder()
    }
    #[cfg(not(windows))]
    {
        let _ = current;
        None
    }
}

/// Trabalho pesado fora da UI (thread de decode).
enum Job {
    List(String),
    Decode(PathBuf),
    Exif(PathBuf),
    /// Export de privacidade: re-encoda girado `deg` (múltiplo de 90 — a
    /// rotação corrente do viewer) como PNG ao lado do original
    /// (`nome.lab.png`) — EXIF nunca sobrevive.
    Export(PathBuf, u32),
}

enum Done {
    List(Result<Vec<PathBuf>, String>),
    Decode { path: PathBuf, result: Result<(egui::ColorImage, Option<img::ImageInfo>), String> },
    Exif(Vec<(String, String)>),
    Export(Result<PathBuf, String>),
}

fn spawn_job(job: Job, tx: Sender<Done>) {
    std::thread::spawn(move || match job {
        Job::List(dir) => {
            let _ = tx.send(Done::List(img::list_dir(&dir)));
        }
        Job::Decode(path) => {
            // Dimensões/tamanho sem decodificar de novo (cabeçalho).
            let info = img::image_info(&path).ok();
            let result = image::open(&path).map(|im| {
                let rgba = im.to_rgba8();
                (egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()), info)
            });
            let _ = tx.send(Done::Decode { path, result: result.map_err(|e| e.to_string()) });
        }
        Job::Exif(path) => {
            let _ = tx.send(Done::Exif(img::exif_info(&path)));
        }
        Job::Export(src, deg) => {
            let dst = src.with_extension("lab.png");
            let _ = tx.send(Done::Export(img::export(&src, &dst, deg, None, 90).map(|_| dst)));
        }
    });
}

struct ImageApp {
    cfg: Config,
    dir: String,
    files: Vec<PathBuf>,
    idx: Option<usize>,
    /// Última imagem decodificada (path → textura) — trocou de imagem,
    /// trocou de textura (sem cache infinito: RAM manda).
    tex: Option<(PathBuf, egui::TextureHandle)>,
    exif: Option<Vec<(String, String)>>,
    /// Canal geral: List / Exif / Export.
    rx: Option<Receiver<Done>>,
    /// Canal dedicado do Decode — não pode ser sobrescrito pelo Exif.
    decode_rx: Option<Receiver<Done>>,
    status: String,
    // viewport do zoom
    zoom: f32,
    pan: egui::Vec2,
    fit: bool,
    /// Pedido de "centralizar mantendo o zoom" (tecla C / botão ⊙): o offset
    /// depende do `avail` do painel central, então é consumido lá dentro.
    center_pending: bool,
    show_exif: bool,
    /// Rotação do viewer em quartos de volta horário (0..3). Persiste na
    /// sessão (varredura de fotos giradas não re-gira a cada foto).
    rot: u32,
    /// Se o app foi aberto com um arquivo via args, guardamos o path até
    /// a lista da pasta estar pronta (decode é async via channel).
    initial_file: Option<PathBuf>,
    /// Interface visível (toolbar/rodapé). "Abrir com" abre limpa (só a
    /// imagem); clique na imagem ou F traz de volta.
    chrome: bool,
    /// Ajuste único de escala/tamanho da janela após o primeiro frame
    /// (corrige ppp != 1.0 e clampa imagens maiores que a tela).
    window_fix: Option<[f32; 2]>,
    /// Viewport vista no frame anterior (largura×altura do `inner_rect`) —
    /// detecta o resize da tela cheia e agenda o frame extra de assentamento.
    last_vp: Option<[f32; 2]>,
    /// Preencher a tela (cover) pendente do F que ENTROU em fullscreen: o
    /// zoom final depende do `avail` da viewport JÁ em tela cheia, então
    /// fica pendente e é aplicado no painel central até a viewport assentar.
    fs_cover: bool,
    /// Verificação e instalação de atualização (craft_update): check no
    /// arranque/pedido, prompt egui próprio e self-update em thread própria.
    updater: craft_update::updater::Updater,
}

impl ImageApp {
    fn new(cfg: Config, initial_file: Option<PathBuf>, opened_with_file: bool) -> Self {
        Self {
            cfg,
            dir: String::new(),
            files: Vec::new(),
            idx: None,
            tex: None,
            exif: None,
            rx: None,
            decode_rx: None,
            status: String::new(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit: true,
            center_pending: false,
            show_exif: false,
            rot: 0,
            initial_file: initial_file.clone(),
            chrome: !opened_with_file,
            window_fix: initial_file.as_ref().and_then(|p| image::image_dimensions(p).ok()).map(|(w, h)| [w as f32, h as f32]),
            last_vp: None,
            fs_cover: false,
            updater: craft_update::updater::Updater::new(&craft_update::suite::LOCALIMAGE, CURRENT_VERSION),
        }
    }

    /// Verifica atualizações agora (botão "Verificar atualizações"). O
    /// resultado sai na barra de status do rodapé; release nova abre o
    /// prompt do próprio updater.
    fn check_updates(&mut self, ctx: &egui::Context) {
        self.updater.check_now(ctx);
    }

    /// Enfileira um job nos canais genéricos (List / Exif / Export).
    fn request(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.rx = Some(rx);
    }

    /// Enfileira um decode — canal dedicado para não ser sobrescrito.
    fn request_decode(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.decode_rx = Some(rx);
    }

    fn current(&self) -> Option<&PathBuf> {
        // `.get` e não indexação: a lista pode ser re-listada pelo rodapé por
        // baixo do viewer, e um `idx` velho estouraria.
        self.idx.and_then(|i| self.files.get(i))
    }

    fn goto(&mut self, i: usize) {
        if i < self.files.len() {
            self.idx = Some(i);
            self.tex = None;
            self.exif = None;
            self.refit();
            let path = self.files[i].clone();
            // Vídeo: a image crate não abre contêiner — decodificar daria
            // erro e o Exif giraria pra sempre. Abre no player do sistema e
            // salta pra próxima IMAGEM à frente (senão o viewer pararia no
            // primeiro vídeo da pasta); sem imagem à frente, fica no vídeo
            // com o placeholder do painel central.
            if img::is_video(&path) {
                self.status = match open_path(&path) {
                    Ok(()) => format!("▶ {}", i18n::t(self.cfg.lang, Key::VideoHint)),
                    Err(e) => format!("⚠ {e}"),
                };
                // Busca num `let` separado: o iterador empresta `files` e
                // não pode viver até o `self.goto` (que pede &mut self).
                let next = (i + 1..self.files.len()).find(|j| img::is_image(&self.files[*j]));
                if let Some(next) = next {
                    self.goto(next);
                }
                return;
            }
            self.request_decode(Job::Decode(path.clone()));
            self.request(Job::Exif(path));
        }
    }

    /// Abre a pasta do rodapé do viewer (o campo binda em `self.dir`, que
    /// ali é sempre o caminho da pasta aberta — limpo no Enter). O caminho
    /// passa pelo saneamento: colar do "Copiar como caminho" do Windows traz
    /// aspas em volta.
    fn open_dir(&mut self) {
        let dir = img::sanitize_path_input(&self.dir);
        if dir.is_empty() {
            return;
        }
        // Devolve o caminho limpo pro campo: o usuário vê o que abriu.
        self.dir = dir.clone();
        // Pasta nova = o viewer recomeça: sem o reset, o open_first do dreno
        // não dispara (idx ocupado) e o viewer ficaria no idx da pasta
        // anterior.
        self.idx = None;
        self.tex = None;
        self.exif = None;
        self.request(Job::List(dir));
    }

    /// Volta pro modo "encaixar" (contain). Toda transição manual→fit precisa
    /// zerar o zoom junto: `fit` desenha com `base_scale * zoom`, e um zoom
    /// residual (scroll/drag anteriores) fazia a imagem sair do encaixe
    /// duplicada/truncada (bug 0.2.0: sair do fullscreen, C, mover).
    fn refit(&mut self) {
        self.fit = true;
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        self.center_pending = false;
    }
}

/// Tamanho exibido da textura com rotação aplicada (90°/270° trocam eixos).
fn displayed_size(tex: &egui::TextureHandle, rot: u32) -> egui::Vec2 {
    let s = tex.size_vec2();
    if rot % 2 == 1 {
        egui::vec2(s.y, s.x)
    } else {
        s
    }
}

/// Pinta a textura em `dest` girada em quartos de volta horário — mesh
/// com os cantos do UV permutados (`painter.image` só mapeia rect→rect,
/// rotação de 90° não é expressável assim).
fn paint_rotated(painter: &egui::Painter, tex: egui::TextureId, dest: egui::Rect, rot: u32) {
    let white = egui::Color32::WHITE;
    let (dtl, utl) = (dest.left_top(), egui::pos2(0.0, 0.0));
    let (dtr, utr) = (dest.right_top(), egui::pos2(1.0, 0.0));
    let (dbr, ubr) = (dest.right_bottom(), egui::pos2(1.0, 1.0));
    let (dbl, ubl) = (dest.left_bottom(), egui::pos2(0.0, 1.0));
    // Rotação horário: o pixel do canto superior-esquerdo da textura vai
    // parar no canto superior-direito do destino (e segue a volta).
    let v = |pos: egui::Pos2, uv: egui::Pos2| egui::epaint::Vertex { pos, uv, color: white };
    let quad = match rot % 4 {
        0 => [v(dtl, utl), v(dtr, utr), v(dbr, ubr), v(dbl, ubl)],
        1 => [v(dtl, ubl), v(dtr, utl), v(dbr, utr), v(dbl, ubr)],
        2 => [v(dtl, ubr), v(dtr, ubl), v(dbr, utl), v(dbl, utr)],
        _ => [v(dtl, utr), v(dtr, ubr), v(dbr, ubl), v(dbl, utl)],
    };
    let mut mesh = egui::Mesh::default();
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh.vertices = quad.to_vec();
    mesh.texture_id = tex;
    painter.add(egui::Shape::mesh(mesh));
}

/// Placeholder do painel central quando o arquivo atual é vídeo (o viewer
/// não decodifica contêiner — o vídeo foi pro player do sistema): hint
/// traduzido + nome do arquivo, centrados, em tinta fraca. Desenhado à mão
/// porque `centered_and_justified` só aceita UM widget e aqui são duas
/// linhas de tamanhos diferentes. O `rect` vem do caller (a área do clique
/// que revela a interface).
fn video_placeholder(ui: &mut egui::Ui, rect: egui::Rect, hint: &str, name: &str) {
    let painter = ui.painter_at(rect);
    let weak = ui.visuals().weak_text_color();
    let f_hint = egui::TextStyle::Body.resolve(ui.style());
    let f_name = egui::TextStyle::Small.resolve(ui.style());
    let gap = ui.spacing().item_spacing.y;
    let hint_galley = painter.layout(hint.to_owned(), f_hint, weak, rect.width());
    let name_galley = painter.layout(name.to_owned(), f_name, weak, rect.width());
    // Bloco centrado na área do viewer: alturas medidas dos próprios galleys
    // (sem constante mágica de "altura de duas linhas").
    let total_h = hint_galley.size().y + gap + name_galley.size().y;
    let top = rect.center().y - total_h / 2.0;
    let hint_pos = egui::pos2(rect.center().x - hint_galley.size().x / 2.0, top);
    let name_pos = egui::pos2(rect.center().x - name_galley.size().x / 2.0, top + hint_galley.size().y + gap);
    painter.galley(hint_pos, hint_galley, weak);
    painter.galley(name_pos, name_galley, weak);
}

impl eframe::App for ImageApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();

        // Updater (craft_update): drena o check/instalação pendente (o tick
        // acorda a UI por si). Resultado de um check PEDIDO sai na barra de
        // status do rodapé; release nova abre o prompt do updater lá embaixo.
        self.updater.tick(ctx);
        match self.updater.status().clone() {
            craft_update::updater::Status::UpToDate => {
                self.status = i18n::t(self.cfg.lang, Key::UpToDate).to_string();
                self.updater.dismiss_status();
            }
            craft_update::updater::Status::Failed(e) => {
                self.status = format!("⚠ {e}");
                self.updater.dismiss_status();
            }
            _ => {}
        }
        // Se abriu com um arquivo via args, dispara o scan da pasta agora.
        if let Some(ref p) = self.initial_file {
            if self.dir.is_empty() {
                if let Some(parent) = p.parent() {
                    let dir = parent.to_string_lossy().to_string();
                    self.dir = dir.clone();
                    self.request(Job::List(dir));
                    ctx.request_repaint();
                    return;
                }
            }
        }

        // Fullscreen/resize: o tamanho da viewport muda FORA do egui, como
        // evento do SO. Com `fit` ativo o encaixe é recalculado todo frame,
        // mas o frame do próprio evento pode ainda ver o tamanho velho —
        // um frame EXTRA logo após a mudança garante a re-avaliação com a
        // área já assentada (sem isso o app idle não repinta de novo).
        let mut vp_changed = false;
        let vp = ctx.input(|i| {
            let r = i.viewport().inner_rect.unwrap_or_else(|| i.raw.screen_rect.unwrap_or(egui::Rect::NOTHING));
            [r.width(), r.height()]
        });
        if self.last_vp.is_none() {
            self.last_vp = Some(vp); // baseline do boot
        } else if self.last_vp != Some(vp) {
            self.last_vp = Some(vp);
            vp_changed = true;
            if self.fit || self.fs_cover {
                ctx.request_repaint();
            }
        }

        // Ajuste da janela pro tamanho da imagem ("Abrir com" no boot e
        // tecla R): px→points, clamp na área útil (taskbar descontada) e
        // centralização — helper compartilhado do lab-ui (workarea.rs).
        if let Some([w, h]) = self.window_fix.take() {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if !fullscreen {
                let (inner, pos) = fit_to_work_area(ctx, w, h);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }

        // Atalhos de LETRA só fora de campo de texto (o rodapé tem o
        // caminho da pasta: digitar "f" não pode disparar tela cheia).
        let kb_free = !ctx.egui_wants_keyboard_input();

        // R = redimensiona a janela pra imagem ATUAL (abriu com uma,
        // navegou pras setas — a janela acompanha a nova). Dims já com a
        // rotação aplicada (90° troca os eixos).
        if kb_free && ctx.input(|i| i.key_pressed(egui::Key::R)) {
            let sz = self.tex.as_ref().map(|(_, t)| displayed_size(t, self.rot));
            if let Some(s) = sz {
                self.window_fix = Some([s.x, s.y]);
                // O resize roda no PRÓXIMO frame — sem isso o app idle não
                // repinta e o comando nunca executa.
                ctx.request_repaint();
            }
        }

        // [ / ] = girar 90° (anti-horário / horário) — a rotação é do
        // VIEWER (pan/zoom/fit seguem as dimensões giradas; o export
        // usa o mesmo ângulo).
        if kb_free && self.tex.is_some() {
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                self.rot = (self.rot + 3) % 4;
                self.refit();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                self.rot = (self.rot + 1) % 4;
                self.refit();
            }
        }

        // Drena resultados das threads; `goto(0)` fica fora do borrow.
        let mut open_first = false;
        // `goto` de revalidação da lista (ver o braço `List(Ok)`).
        let mut revalidate_goto: Option<usize> = None;
        if let Some(rx) = &self.rx {
            while let Ok(done) = rx.try_recv() {
                match done {
                    Done::List(Ok(files)) => {
                        self.files = files;
                        self.status.clear();
                        // `idx` ocupado = o usuário já está numa foto: um List
                        // atrasado (rodapé, "Abrir com") não pode pular ele de
                        // volta pra foto 0.
                        if !self.files.is_empty() && self.idx.is_none() {
                            open_first = true;
                        }
                        // A lista pode ter mudado por baixo do viewer (a
                        // pasta foi re-listada pelo rodapé): revalida o índice
                        // e a textura — foto removida vira "sem foto" e
                        // textura de outro arquivo é redecodificada. O `goto`
                        // fica pra DEPOIS do laço: dentro dele o `self.rx`
                        // ainda está emprestado.
                        if self.idx.is_some_and(|i| i >= self.files.len()) {
                            self.idx = None;
                            self.tex = None;
                            self.exif = None;
                        } else if let Some(path) = self.current().cloned() {
                            let stale = match &self.tex {
                                Some((t, _)) => t != &path,
                                None => true,
                            };
                            if stale && img::is_image(&path) {
                                revalidate_goto = self.idx;
                            }
                        }
                    }
                    Done::List(Err(e)) => {
                        // Só o aviso no rodapé — nada de recuperação de campo
                        // (o campo da pasta segue ali, sempre visível). A
                        // lista em memória sai junto pra o viewer cair no
                        // estado vazio em vez de girar spinner numa pasta que
                        // não abriu. O `initial_file` some pra um boot com
                        // caminho ruim não reapontar a foto depois.
                        self.status = format!("⚠ {e}");
                        self.files.clear();
                        self.initial_file = None;
                    }
                    Done::Exif(entries) => self.exif = Some(entries),
                    Done::Export(Ok(dst)) => {
                        self.status = format!("✓ {}", dst.display());
                    }
                    Done::Export(Err(e)) => self.status = format!("⚠ {e}"),
                    // Decode viaja só no canal dedicado (`decode_rx`).
                    Done::Decode { .. } => {}
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.rx = None;
            }
        }
        // Canal dedicado do decode (não compartilhado com Exif). O
        // resultado SÓ entra se ainda for a imagem atual — navegação
        // rápida tinha race: o decode antigo chegava atrasado e
        // sobrescrevia a imagem errada.
        if let Some(rx) = &self.decode_rx {
            while let Ok(done) = rx.try_recv() {
                if let Done::Decode { path, result } = done {
                    if self.current() != Some(&path) {
                        continue; // usuário já navegou pra outra
                    }
                    match result {
                        Ok((image, info)) => {
                            if let Some(i) = info {
                                self.status = format!("{}×{} · {:.1} MB", i.width, i.height, i.size_bytes as f64 / 1e6);
                            }
                            let tex = ctx.load_texture("view", image, egui::TextureOptions::default());
                            self.tex = Some((path, tex));
                        }
                        Err(e) => self.status = format!("⚠ {e}"),
                    }
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.decode_rx = None;
            }
        }
        if open_first {
            if let Some(ref target) = self.initial_file {
                if let Some(i) = self.files.iter().position(|f| f == target) {
                    self.goto(i);
                } else {
                    self.goto(0);
                }
            } else {
                self.goto(0);
            }
            self.initial_file = None;
        }
        // Textura de outro arquivo (lista trocou por baixo do viewer):
        // redecodifica a foto do índice corrente.
        if let Some(i) = revalidate_goto {
            self.goto(i);
        }
        // Repaint enquanto QUALQUER job estiver no ar. Antes só o canal
        // genérico (rx) agendava: se o Exif terminasse antes do decode, o
        // app ficava idle e a imagem só aparecia quando o usuário movia
        // o mouse — o "às vezes demora" do viewer.
        if self.rx.is_some() || self.decode_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        // Atalhos de navegação (mesmos do oficial).
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if let Some(i) = self.idx {
                if i > 0 {
                    self.goto(i - 1);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if let Some(i) = self.idx {
                if i + 1 < self.files.len() {
                    self.goto(i + 1);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.chrome && self.show_exif {
                self.show_exif = false;
            } else {
                // Só manda o comando quando está em tela cheia (idem toggle
                // abaixo: sair dela muda a área → re-encaixa a imagem).
                let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                if fs {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    self.refit();
                    self.fs_cover = false;
                    ctx.request_repaint();
                }
            }
        }
        // F = tela cheia; 0 = encaixar. ENTRAR preenche a tela (cover):
        // sai do fit e deixa o zoom "cover" pendente — o avail da viewport
        // em tela cheia só existe DEPOIS que o SO aplica o comando, então
        // é consumido no painel central (o watch de last_vp agenda o frame
        // extra da viewport assentada). SAIR volta pro fit (contain).
        // Alternar a tela cheia muda a viewport FORA do egui: sem repaint
        // agendado o app idle não refaz o layout e a imagem continuava no
        // tamanho da janela antiga.
        if kb_free && ctx.input(|i| i.key_pressed(egui::Key::F)) {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
            if fs {
                self.refit();
                self.fs_cover = false;
            } else {
                self.fit = false;
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
                self.fs_cover = true;
            }
            ctx.request_repaint();
        }
        // C = centralizar mantendo o zoom (o offset materializa como pan,
        // igual ao primeiro scroll saindo do fit); 0 = fit completo.
        if kb_free && self.tex.is_some() && ctx.input(|i| i.key_pressed(egui::Key::C)) {
            self.center_pending = true;
        }
        if kb_free && ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
            self.refit();
        }
        // i = painel EXIF (mesma guarda de letra: digitar "i" no campo do
        // rodapé não pode abrir o painel).
        if kb_free && ctx.input(|i| i.key_pressed(egui::Key::I)) {
            self.show_exif = !self.show_exif;
        }

        // Interface: toolbar/rodapé só quando `chrome`.
        let chrome = self.chrome;
        if chrome {
            egui::containers::Panel::top("topo").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Local Image");
                    let n = self.idx.map(|i| format!("{}/{}", i + 1, self.files.len())).unwrap_or_default();
                    ui.label(egui::RichText::new(n).small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (cfg_changed, _) = lab_ui::settings_ui(ui, &mut self.cfg, false);
                        if cfg_changed {
                            theme::apply(ctx, self.cfg.theme);
                            let _ = config::save(APP_ID, &self.cfg);
                        }
                        if ghost(ui, i18n::t(self.cfg.lang, Key::CheckUpdates)).clicked() {
                            self.check_updates(ctx);
                        }
                        if self.tex.is_some() && ghost(ui, "⟳").on_hover_text(i18n::t(self.cfg.lang, Key::RotateHint)).clicked() {
                            self.rot = (self.rot + 1) % 4;
                            self.refit();
                        }
                        // Export de privacidade em texto: o glifo "⤴" não
                        // dizia nada e o painel EXIF agora é a tecla "i".
                        // A guarda acompanha o ⟳/⊙ ao lado (`tex`): vídeo ou
                        // decode no ar só dariam "⚠ decodificar" ao clicar.
                        if self.tex.is_some()
                            && pill(ui, i18n::t(self.cfg.lang, Key::ExportClean), false)
                                .on_hover_text(i18n::t(self.cfg.lang, Key::ExportCleanHint))
                                .clicked()
                        {
                            if let Some(p) = self.current() {
                                let p = p.clone();
                                self.request(Job::Export(p, (self.rot % 4) * 90));
                            }
                        }
                        if self.tex.is_some() && ghost(ui, "⊙").on_hover_text(i18n::t(self.cfg.lang, Key::CenterHint)).clicked() {
                            self.center_pending = true;
                        }
                    });
                });
            });
        }

        if chrome {
            egui::containers::Panel::bottom("rodape").show(ui, |ui| {
                // Direita→esquerda: o pill entra PRIMEIRO (canto direito) e o
                // campo vem depois preenchendo o resto. Na ordem natural o
                // `desired_width(INFINITY)` do campo tomava a largura toda e
                // empurrava o "abrir" pra fora da janela.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pill(ui, i18n::t(self.cfg.lang, Key::Open), true).clicked() {
                            self.open_dir();
                        }
                        // `align`: o campo vive num layout da direita pra
                        // esquerda, mas caminho é conteúdo LTR — sem isso o
                        // texto digitado encostava à direita.
                        let resp =
                            ui.add(line(&mut self.dir).hint_text(r"C:\...\fotos").align(egui::Align2::LEFT_CENTER).desired_width(f32::INFINITY));
                        // Enter abre a pasta: o TextEdit singleline perde o
                        // foco no Enter, então o pedido chega como lost_focus.
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.open_dir();
                        }
                    });
                });
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
            });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.files.is_empty() {
                // Estado vazio (boot sem arquivo): a AÇÃO principal é escolher
                // a pasta no seletor NATIVO, com o hint fraco embaixo. Clicar
                // na área revela a interface — sem isso uma janela nascida
                // limpa ficaria sem toolbar e sem rodapé.
                let area = ui.allocate_response(ui.available_size(), egui::Sense::click());
                let mut pick = false;
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.rect).layout(egui::Layout::top_down(egui::Align::Center)));
                // Bloco desce ~1/3 da área livre (proporcional: escala com a
                // janela, sem altura mágica).
                child.add_space(area.rect.height() * 0.36);
                if NATIVE_PICKER && pill(&mut child, i18n::t(self.cfg.lang, Key::OpenFolder), true).clicked() {
                    pick = true;
                }
                child.add_space(10.0);
                child.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::PickFolder)).weak());
                // Pasta escolhida no seletor NATIVO: aplicada depois do
                // desenho (durante ele o diálogo bloquearia o frame); `None` =
                // o usuário cancelou: silêncio.
                if pick {
                    if let Some(chosen) = pick_folder(&self.dir) {
                        self.dir = chosen.to_string_lossy().into_owned();
                        self.open_dir();
                    }
                } else if area.clicked() {
                    self.chrome = true;
                }
                return;
            }

            let screen = ui.available_size();
            // Painel EXIF é uma Window ancorada (clip/scroll grátis); aqui
            // só reservamos o espaço pra ela não cobrir a imagem.
            let exif_w = if self.show_exif { 240.0 } else { 0.0 };
            let avail = egui::vec2(screen.x - exif_w, screen.y);

            if let Some((_, tex)) = &self.tex {
                // Dimensões exibidas: rotação de 90°/270° troca os eixos
                // (fit/zoom/pan trabalham no espaço girado).
                let size = displayed_size(tex, self.rot);

                // Cover do fullscreen: o frame do F ainda vê a área da
                // janela velha — aplica o zoom "preencher" nos frames em
                // que a viewport muda e para quando ela assenta.
                if self.fs_cover {
                    let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if fs && vp_changed {
                        // base_scale é 1.0 com fit=false: o zoom É a
                        // escala final — cover = maior razão dos eixos.
                        self.zoom = (avail.x / size.x).max(avail.y / size.y);
                        self.pan = egui::Vec2::ZERO;
                        ctx.request_repaint(); // um frame a mais pra fechar o pedido
                    } else if fs {
                        self.fs_cover = false; // assentou — para de aplicar
                    }
                }

                let (base_scale, base_off) = if self.fit {
                    let s = (avail.x / size.x).min(avail.y / size.y).min(1.0);
                    let scaled = size * s;
                    (s, egui::vec2((avail.x - scaled.x) / 2.0, (avail.y - scaled.y) / 2.0))
                } else {
                    (1.0, egui::Vec2::ZERO)
                };

                let response = ui.allocate_response(avail, egui::Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
                let rect = response.rect;

                // C centraliza SEM trocar o zoom VISUAL: sai do fit
                // materializando a escala corrente como zoom (o draw de
                // fora do fit é `1.0 * zoom` — sem isso o zoom caía pra 1.0
                // e a imagem pulava pro tamanho nativo, bug 0.2.0).
                if self.center_pending {
                    self.center_pending = false;
                    if self.fit {
                        self.zoom *= base_scale;
                        self.fit = false;
                    }
                    let scale = self.zoom;
                    self.pan = egui::vec2((avail.x - size.x * scale) / 2.0, (avail.y - size.y * scale) / 2.0);
                }

                // Zoom pela roda, ancorado no cursor (o "natural"): o
                // ponto da imagem que está sob o mouse fica lá. Sair do fit
                // MATERIALIZA escala e centralização (zoom = base*zoom,
                // pan = base_off) — sem isso o primeiro scroll pulava pro
                // zoom 100% (bug 0.2.0).
                let scroll = response.hovered() && ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.0);
                if scroll {
                    if self.fit {
                        self.pan = base_off;
                        self.zoom *= base_scale;
                        self.fit = false;
                    }
                    let factor = if ui.input(|i| i.smooth_scroll_delta.y > 0.0) { 1.1 } else { 1.0 / 1.1 };
                    let new_zoom = (self.zoom * factor).clamp(0.05, 40.0);
                    if let Some(m) = response.hover_pos() {
                        let m = m - rect.min; // cursor na área do viewer
                                              // p_img = (m - pan)/zoom fica fixo em m (base é 1.0 fora do fit):
                        self.pan = m - (m - self.pan) / self.zoom * new_zoom;
                    }
                    self.zoom = new_zoom;
                }

                // Pan com arrastar. Primeiro arraste também materializa
                // (mesma razão do scroll — o draw fora do fit é zoom*1.0).
                if response.dragged() {
                    if self.fit {
                        self.pan = base_off;
                        self.zoom *= base_scale;
                        self.fit = false;
                    }
                    self.pan += response.drag_delta();
                }
                // Clique simples (sem drag) alterna a interface.
                if response.clicked() {
                    self.chrome = !self.chrome;
                }
                if response.double_clicked() {
                    // (refit inline: `tex` empresta self aqui em cima)
                    self.fit = true;
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                    self.center_pending = false;
                }

                // Base pós-interações: se uma transição fit→manual aconteceu
                // ACIMA, o draw deste frame tem que usar a base nova (1.0)
                // senão escala dupla por um frame (flash).
                let (base_scale, base_off) = if self.fit {
                    let scaled = size * base_scale;
                    (base_scale, egui::vec2((avail.x - scaled.x) / 2.0, (avail.y - scaled.y) / 2.0))
                } else {
                    (1.0, egui::Vec2::ZERO)
                };

                let scale = base_scale * self.zoom;
                let dest = egui::Rect::from_min_size(rect.min + base_off + self.pan, size * scale);
                let painter = ui.painter_at(rect);
                paint_rotated(&painter, tex.id(), dest, self.rot);
            } else if self.current().is_some_and(|p| img::is_video(p)) {
                // Vídeo (sem imagem à frente): em vez de spinner eterno, a
                // informação útil — o que aconteceu e qual arquivo. Clique
                // revela a interface (mesma saída do estado vazio: sem isso,
                // "abrir com" num vídeo de pasta só-de-vídeos deixaria a
                // janela sem NENHUMA forma de trazer o chrome de volta).
                let resp = ui.allocate_response(ui.available_size(), egui::Sense::click());
                let hint = i18n::t(self.cfg.lang, Key::VideoHint);
                let name = self.current().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                video_placeholder(ui, resp.rect, hint, &name);
                if resp.clicked() {
                    self.chrome = true;
                }
            } else {
                // Spinner SÓ para imagem decodificando.
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            }
        });

        // Painel EXIF do viewer.
        if self.show_exif {
            egui::Window::new("EXIF")
                .id(egui::Id::unique("exif"))
                .frame(modal_frame(ctx))
                .default_open(true)
                .collapsible(true)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_TOP, [-8.0, 56.0])
                .show(ctx, |ui| {
                    begin_dialog(ui);
                    match &self.exif {
                        Some(entries) if entries.is_empty() => {
                            ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::NoExif)).weak());
                        }
                        Some(entries) => {
                            egui::Grid::new("exif").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                                for (k, v) in entries {
                                    ui.label(egui::RichText::new(k).small().weak());
                                    ui.label(v);
                                    ui.end_row();
                                }
                            });
                        }
                        None => {
                            // Sem imagem não há EXIF chegando; vídeo também
                            // nunca dispara o Exif (`goto` pula) — nos dois
                            // casos o spinner giraria pra sempre.
                            let cur = self.current();
                            if cur.is_none() {
                                ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::NoImage)).weak());
                            } else if cur.is_some_and(|p| img::is_video(p)) {
                                ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::NoExif)).weak());
                            } else {
                                ui.spinner();
                            }
                        }
                    }
                });
        }

        // Prompt de update (craft_update): por cima de tudo. "Atualizar e
        // reiniciar" pede o fechamento pelo caminho limpo (ViewportCommand::
        // Close) — o helper do craft_update instala e relança depois que o
        // app sai.
        match self.updater.show(ctx) {
            craft_update::updater::Action::None => {}
            craft_update::updater::Action::OpenUrl(url) => open_url(&url),
            craft_update::updater::Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }

        // Repaint de novo no FIM: este `ui()` roda antes do próximo dreno, e
        // um job que chegou durante o desenho só seria visto no frame
        // seguinte. Sem este segundo agendamento o app idle ficava parado até
        // o mouse se mexer.
        if self.rx.is_some() || self.decode_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
}
