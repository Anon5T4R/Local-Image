//! Local Image â€” visualizador de imagens 100% local e offline.
//! Viewer + EXIF + export re-encodado (privacidade: EXIF nunca sobrevive).
//! NÃºcleo (`img.rs`) portado do LocalImage oficial; promovido do lab-image
//! (egui-lab) pra repo standalone da suÃ­te Local.
//!
//! VisualizaÃ§Ã£o: a image crate decodifica â†’ textura egui; zoom por roda do
//! mouse com Ã¢ncora no cursor, pan com arrastar, encaixe na janela (tecla
//! 0), centralizar mantendo o zoom (tecla C), tela cheia preenchendo a tela
//! (F), janela no tamanho da imagem (R), setas â†/â†’ navegam a pasta.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod img;
mod ui;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use lab_ui::workarea::fit_to_work_area;
use ui::{begin_dialog, ghost, line, modal_frame, pill};

const APP_ID: &str = "local-image";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ãcone da janela (alÃ©m do .exe cunhado no build.rs â€” Windows pega os
/// dois; AppImage/desktop sÃ³ vendo com o PNG do pacote).
const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Decodifica o PNG do Ã­cone pra `IconData` (o `.with_icon` do egui).
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = img.to_rgba8().into_raw();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba })
}

fn main() -> eframe::Result<()> {
    // Windows: o helper de update Ã© uma CÃ“PIA deste programa iniciada com
    // `--finish-update` (craft_update). Roda ANTES de tudo.
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::LOCALIMAGE, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }

    let cfg = config::load(APP_ID);

    // "Abrir com" do Windows manda o caminho como primeiro arg.
    // canonicalize garante que o path bate com o que read_dir retorna.
    let initial_file = std::env::args().nth(1).map(PathBuf::from).and_then(|p| p.canonicalize().ok());

    // "Abrir com" = janela no tamanho REAL da imagem (header only â€” sem
    // decode). ppp ainda Ã© desconhecido aqui (points â‰ˆ px em 100%); o
    // primeiro update corrige a escala e clampa na tela.
    let inner_size = initial_file.as_ref().and_then(|p| image::image_dimensions(p).ok()).map(|(w, h)| [w as f32, h as f32]).unwrap_or([900.0, 640.0]);

    let mut viewport = egui::ViewportBuilder::default().with_title("Local Image").with_inner_size(inner_size);
    // Ãcone da janela (o .exe jÃ¡ cunha o mesmo no build.rs â€” Resources).
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

/// Abre a pÃ¡gina da release no navegador â€” aÃ§Ã£o "Download" do prompt do
/// updater, para instalaÃ§Ãµes que nÃ£o se auto-atualizam (zip solto, dev build).
fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb = HSTRING::from("open");
        let file = HSTRING::from(url);
        // > 32 = lanÃ§ou.
        let res = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if res.0 as usize <= 32 {
            eprintln!("update: falha ao abrir {url} (cÃ³digo {})", res.0 as usize);
        }
    }
    #[cfg(not(windows))]
    {
        if std::process::Command::new("xdg-open").arg(url).spawn().is_err() {
            eprintln!("update: falha ao abrir {url}");
        }
    }
}

/// Trabalho pesado fora da UI (thread de decode).
enum Job {
    List(String),
    Decode(PathBuf),
    Exif(PathBuf),
    /// Export de privacidade: re-encoda girado `deg` (mÃºltiplo de 90 â€” a
    /// rotaÃ§Ã£o corrente do viewer) como PNG ao lado do original
    /// (`nome.lab.png`) â€” EXIF nunca sobrevive.
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
            // DimensÃµes/tamanho sem decodificar de novo (cabeÃ§alho).
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
    /// Ãšltima imagem decodificada (path â†’ textura) â€” trocou de imagem,
    /// trocou de textura (sem cache infinito: RAM manda).
    tex: Option<(PathBuf, egui::TextureHandle)>,
    exif: Option<Vec<(String, String)>>,
    /// Canal geral: List / Exif / Export.
    rx: Option<Receiver<Done>>,
    /// Canal dedicado do Decode â€” nÃ£o pode ser sobrescrito pelo Exif.
    decode_rx: Option<Receiver<Done>>,
    status: String,
    // viewport do zoom
    zoom: f32,
    pan: egui::Vec2,
    fit: bool,
    /// Pedido de "centralizar mantendo o zoom" (tecla C / botÃ£o âŠ™): o offset
    /// depende do `avail` do painel central, entÃ£o Ã© consumido lÃ¡ dentro.
    center_pending: bool,
    show_exif: bool,
    /// RotaÃ§Ã£o do viewer em quartos de volta horÃ¡rio (0..3). Persiste na
    /// sessÃ£o (varredura de fotos giradas nÃ£o re-gira a cada foto).
    rot: u32,
    /// Se o app foi aberto com um arquivo via args, guardamos o path atÃ©
    /// a lista da pasta estar pronta (decode Ã© async via channel).
    initial_file: Option<PathBuf>,
    /// Interface visÃ­vel (toolbar/rodapÃ©). "Abrir com" abre limpa (sÃ³ a
    /// imagem); clique na imagem ou F traz de volta.
    chrome: bool,
    /// Ajuste Ãºnico de escala/tamanho da janela apÃ³s o primeiro frame
    /// (corrige ppp != 1.0 e clampa imagens maiores que a tela).
    window_fix: Option<[f32; 2]>,
    /// Viewport vista no frame anterior (larguraÃ—altura do `inner_rect`) â€”
    /// detecta o resize da tela cheia e agenda o frame extra de assentamento.
    last_vp: Option<[f32; 2]>,
    /// Preencher a tela (cover) pendente do F que ENTROU em fullscreen: o
    /// zoom final depende do `avail` da viewport JÃ em tela cheia, entÃ£o
    /// fica pendente e Ã© aplicado no painel central atÃ© a viewport assentar.
    fs_cover: bool,
    /// VerificaÃ§Ã£o e instalaÃ§Ã£o de atualizaÃ§Ã£o (craft_update): check no
    /// arranque/pedido, prompt egui prÃ³prio e self-update em thread prÃ³pria.
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

    /// Verifica atualizaÃ§Ãµes agora (botÃ£o "Verificar atualizaÃ§Ãµes"). O
    /// resultado sai na barra de status do rodapÃ©; release nova abre o
    /// prompt do prÃ³prio updater.
    fn check_updates(&mut self, ctx: &egui::Context) {
        self.updater.check_now(ctx);
    }

    /// Enfileira um job nos canais genÃ©ricos (List / Exif / Export).
    fn request(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.rx = Some(rx);
    }

    /// Enfileira um decode â€” canal dedicado para nÃ£o ser sobrescrito.
    fn request_decode(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.decode_rx = Some(rx);
    }

    fn current(&self) -> Option<&PathBuf> {
        self.idx.map(|i| &self.files[i])
    }

    fn goto(&mut self, i: usize) {
        if i < self.files.len() {
            self.idx = Some(i);
            self.tex = None;
            self.exif = None;
            self.refit();
            if let Some(p) = self.current() {
                let p = p.clone();
                self.request_decode(Job::Decode(p.clone()));
                self.request(Job::Exif(p));
            }
        }
    }

    /// Volta pro modo "encaixar" (contain). Toda transiÃ§Ã£o manualâ†’fit precisa
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

/// Tamanho exibido da textura com rotaÃ§Ã£o aplicada (90Â°/270Â° trocam eixos).
fn displayed_size(tex: &egui::TextureHandle, rot: u32) -> egui::Vec2 {
    let s = tex.size_vec2();
    if rot % 2 == 1 {
        egui::vec2(s.y, s.x)
    } else {
        s
    }
}

/// Pinta a textura em `dest` girada em quartos de volta horÃ¡rio â€” mesh
/// com os cantos do UV permutados (`painter.image` sÃ³ mapeia rectâ†’rect,
/// rotaÃ§Ã£o de 90Â° nÃ£o Ã© expressÃ¡vel assim).
fn paint_rotated(painter: &egui::Painter, tex: egui::TextureId, dest: egui::Rect, rot: u32) {
    let white = egui::Color32::WHITE;
    let (dtl, utl) = (dest.left_top(), egui::pos2(0.0, 0.0));
    let (dtr, utr) = (dest.right_top(), egui::pos2(1.0, 0.0));
    let (dbr, ubr) = (dest.right_bottom(), egui::pos2(1.0, 1.0));
    let (dbl, ubl) = (dest.left_bottom(), egui::pos2(0.0, 1.0));
    // RotaÃ§Ã£o horÃ¡rio: o pixel do canto superior-esquerdo da textura vai
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

impl eframe::App for ImageApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();

        // Updater (craft_update): drena o check/instalaÃ§Ã£o pendente (o tick
        // acorda a UI por si). Resultado de um check PEDIDO sai na barra de
        // status do rodapÃ©; release nova abre o prompt do updater lÃ¡ embaixo.
        self.updater.tick(ctx);
        match self.updater.status().clone() {
            craft_update::updater::Status::UpToDate => {
                self.status = i18n::t(self.cfg.lang, Key::UpToDate).to_string();
                self.updater.dismiss_status();
            }
            craft_update::updater::Status::Failed(e) => {
                self.status = format!("âš  {e}");
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
        // evento do SO. Com `fit` ativo o encaixe Ã© recalculado todo frame,
        // mas o frame do prÃ³prio evento pode ainda ver o tamanho velho â€”
        // um frame EXTRA logo apÃ³s a mudanÃ§a garante a re-avaliaÃ§Ã£o com a
        // Ã¡rea jÃ¡ assentada (sem isso o app idle nÃ£o repinta de novo).
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
        // tecla R): pxâ†’points, clamp na Ã¡rea Ãºtil (taskbar descontada) e
        // centralizaÃ§Ã£o â€” helper compartilhado do lab-ui (workarea.rs).
        if let Some([w, h]) = self.window_fix.take() {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if !fullscreen {
                let (inner, pos) = fit_to_work_area(ctx, w, h);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }

        // Atalhos de LETRA sÃ³ fora de campo de texto (o rodapÃ© tem o
        // caminho da pasta: digitar "f" nÃ£o pode disparar tela cheia).
        let kb_free = !ctx.egui_wants_keyboard_input();

        // R = redimensiona a janela pra imagem ATUAL (abriu com uma,
        // navegou pras setas â€” a janela acompanha a nova). Dims jÃ¡ com a
        // rotaÃ§Ã£o aplicada (90Â° troca os eixos).
        if kb_free && ctx.input(|i| i.key_pressed(egui::Key::R)) {
            let sz = self.tex.as_ref().map(|(_, t)| displayed_size(t, self.rot));
            if let Some(s) = sz {
                self.window_fix = Some([s.x, s.y]);
                // O resize roda no PRÃ“XIMO frame â€” sem isso o app idle nÃ£o
                // repinta e o comando nunca executa.
                ctx.request_repaint();
            }
        }

        // [ / ] = girar 90Â° (anti-horÃ¡rio / horÃ¡rio) â€” a rotaÃ§Ã£o Ã© do
        // VIEWER (pan/zoom/fit seguem as dimensÃµes giradas; o export
        // usa o mesmo Ã¢ngulo).
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
        if let Some(rx) = &self.rx {
            while let Ok(done) = rx.try_recv() {
                match done {
                    Done::List(Ok(files)) => {
                        self.files = files;
                        self.status.clear();
                        if !self.files.is_empty() {
                            open_first = true;
                        }
                    }
                    Done::List(Err(e)) => self.status = format!("âš  {e}"),
                    Done::Exif(entries) => self.exif = Some(entries),
                    Done::Export(Ok(dst)) => {
                        self.status = format!("âœ“ {}", dst.display());
                    }
                    Done::Export(Err(e)) => self.status = format!("âš  {e}"),
                    _ => {}
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.rx = None;
            }
        }
        // Canal dedicado do decode (nÃ£o compartilhado com Exif). O
        // resultado SÃ“ entra se ainda for a imagem atual â€” navegaÃ§Ã£o
        // rÃ¡pida tinha race: o decode antigo chegava atrasado e
        // sobrescrevia a imagem errada.
        if let Some(rx) = &self.decode_rx {
            while let Ok(done) = rx.try_recv() {
                if let Done::Decode { path, result } = done {
                    if self.current() != Some(&path) {
                        continue; // usuÃ¡rio jÃ¡ navegou pra outra
                    }
                    match result {
                        Ok((image, info)) => {
                            if let Some(i) = info {
                                self.status = format!("{}Ã—{} Â· {:.1} MB", i.width, i.height, i.size_bytes as f64 / 1e6);
                            }
                            let tex = ctx.load_texture("view", image, egui::TextureOptions::default());
                            self.tex = Some((path, tex));
                        }
                        Err(e) => self.status = format!("âš  {e}"),
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
        // Repaint enquanto QUALQUER job estiver no ar. Antes sÃ³ o canal
        // genÃ©rico (rx) agendava: se o Exif terminasse antes do decode, o
        // app ficava idle e a imagem sÃ³ aparecia quando o usuÃ¡rio movia
        // o mouse â€” o "Ã s vezes demora" do viewer.
        if self.rx.is_some() || self.decode_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        // Atalhos de navegaÃ§Ã£o (mesmos do oficial).
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
                // SÃ³ manda o comando quando estÃ¡ em tela cheia (idem toggle
                // abaixo: sair dela muda a Ã¡rea â†’ re-encaixa a imagem).
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
        // sai do fit e deixa o zoom "cover" pendente â€” o avail da viewport
        // em tela cheia sÃ³ existe DEPOIS que o SO aplica o comando, entÃ£o
        // Ã© consumido no painel central (o watch de last_vp agenda o frame
        // extra da viewport assentada). SAIR volta pro fit (contain).
        // Alternar a tela cheia muda a viewport FORA do egui: sem repaint
        // agendado o app idle nÃ£o refaz o layout e a imagem continuava no
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

        // Interface: toolbar/rodapÃ© sÃ³ quando `chrome`.
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
                        if ghost(ui, "i").on_hover_text("EXIF").clicked() {
                            self.show_exif = !self.show_exif;
                        }
                        if self.tex.is_some() && ghost(ui, "âŸ³").on_hover_text("girar 90Â° ([ e ] tambÃ©m giram)").clicked() {
                            self.rot = (self.rot + 1) % 4;
                            self.refit();
                        }
                        if self.current().is_some() && ghost(ui, "â¤´").on_hover_text("export girado (sem EXIF)").clicked() {
                            if let Some(p) = self.current() {
                                let p = p.clone();
                                self.request(Job::Export(p, (self.rot % 4) * 90));
                            }
                        }
                        if self.tex.is_some() && ghost(ui, "âŠ™").on_hover_text("centralizar (C)").clicked() {
                            self.center_pending = true;
                        }
                    });
                });
            });
        }

        if chrome {
            egui::containers::Panel::bottom("rodape").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(line(&mut self.dir).hint_text(r"C:\...\fotos").desired_width(f32::INFINITY));
                    if pill(ui, i18n::t(self.cfg.lang, Key::Open), true).clicked() {
                        let d = self.dir.trim().to_string();
                        if !d.is_empty() {
                            self.request(Job::List(d));
                        }
                    }
                });
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
            });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.files.is_empty() {
                // Sem pasta e sem interface â†’ clique mostra a toolbar
                // (senÃ£o o usuÃ¡rio fica preso).
                let resp = ui
                    .centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("abra uma pasta").weak());
                    })
                    .response;
                if resp.clicked() {
                    self.chrome = true;
                }
                return;
            }

            let screen = ui.available_size();
            // Painel EXIF Ã© uma Window ancorada (clip/scroll grÃ¡tis); aqui
            // sÃ³ reservamos o espaÃ§o pra ela nÃ£o cobrir a imagem.
            let exif_w = if self.show_exif { 240.0 } else { 0.0 };
            let avail = egui::vec2(screen.x - exif_w, screen.y);

            if let Some((_, tex)) = &self.tex {
                // DimensÃµes exibidas: rotaÃ§Ã£o de 90Â°/270Â° troca os eixos
                // (fit/zoom/pan trabalham no espaÃ§o girado).
                let size = displayed_size(tex, self.rot);

                // Cover do fullscreen: o frame do F ainda vÃª a Ã¡rea da
                // janela velha â€” aplica o zoom "preencher" nos frames em
                // que a viewport muda e para quando ela assenta.
                if self.fs_cover {
                    let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if fs && vp_changed {
                        // base_scale Ã© 1.0 com fit=false: o zoom Ã‰ a
                        // escala final â€” cover = maior razÃ£o dos eixos.
                        self.zoom = (avail.x / size.x).max(avail.y / size.y);
                        self.pan = egui::Vec2::ZERO;
                        ctx.request_repaint(); // um frame a mais pra fechar o pedido
                    } else if fs {
                        self.fs_cover = false; // assentou â€” para de aplicar
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
                // fora do fit Ã© `1.0 * zoom` â€” sem isso o zoom caÃ­a pra 1.0
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
                // ponto da imagem que estÃ¡ sob o mouse fica lÃ¡. Sair do fit
                // MATERIALIZA escala e centralizaÃ§Ã£o (zoom = base*zoom,
                // pan = base_off) â€” sem isso o primeiro scroll pulava pro
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
                        let m = m - rect.min; // cursor na Ã¡rea do viewer
                                              // p_img = (m - pan)/zoom fica fixo em m (base Ã© 1.0 fora do fit):
                        self.pan = m - (m - self.pan) / self.zoom * new_zoom;
                    }
                    self.zoom = new_zoom;
                }

                // Pan com arrastar. Primeiro arraste tambÃ©m materializa
                // (mesma razÃ£o do scroll â€” o draw fora do fit Ã© zoom*1.0).
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

                // Base pÃ³s-interaÃ§Ãµes: se uma transiÃ§Ã£o fitâ†’manual aconteceu
                // ACIMA, o draw deste frame tem que usar a base nova (1.0)
                // senÃ£o escala dupla por um frame (flash).
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
            } else {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            }
        });

        if self.show_exif {
            egui::Window::new("EXIF")
                .id(egui::Id::unique("exif"))
                .frame(modal_frame(ctx))
                .default_open(true)
                .collapsible(true)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
                .show(ctx, |ui| {
                    begin_dialog(ui);
                    match &self.exif {
                        Some(entries) if entries.is_empty() => {
                            ui.label(egui::RichText::new("sem EXIF").weak());
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
                            ui.spinner();
                        }
                    }
                });
        }

        // Prompt de update (craft_update): por cima de tudo. "Atualizar e
        // reiniciar" pede o fechamento pelo caminho limpo (ViewportCommand::
        // Close) â€” o helper do craft_update instala e relanÃ§a depois que o
        // app sai.
        match self.updater.show(ctx) {
            craft_update::updater::Action::None => {}
            craft_update::updater::Action::OpenUrl(url) => open_url(&url),
            craft_update::updater::Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}
