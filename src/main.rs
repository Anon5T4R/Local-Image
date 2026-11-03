//! Local Image — visualizador de imagens 100% local e offline.
//! Viewer + galeria + EXIF + export re-encodado (privacidade: EXIF nunca sobrevive).
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
//! Galeria: a home do app quando ele abre SEM arquivo. Breadcrumb + chips de
//! subpastas, grade de miniaturas virtualizada (só as linhas visíveis são
//! desenhadas e só os tiles visíveis pedem miniatura), busca por nome e
//! seleção. Duplo-clique numa foto abre uma INSTÂNCIA nova do app (o app já
//! nasce abrindo o caminho do argv); num vídeo, abre o player do sistema.
//!
//! Gerenciamento de arquivos (galeria): renomear, mover dentro do volume,
//! excluir pra LIXEIRA do sistema, criar pasta e desfazer o que dá (mover/
//! renomear) na sessão — as operações vivem em `fsops.rs`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fsops;
mod img;
mod ui;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key, Lang};
use lab_ui::theme;
use lab_ui::workarea::fit_to_work_area;
use ui::{begin_dialog, ghost, line, modal_frame, pill, pill_danger, Skin};

const APP_ID: &str = "local-image";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Lado do cartão do tile da galeria (a miniatura cabe dentro dele).
const TILE_SIDE: f32 = 140.0;
/// Folga entre o cartão e a borda da célula: o contorno de seleção precisa
/// de ar pra respirar.
const TILE_MARGIN: f32 = 4.0;
/// Altura da linha do nome embaixo do cartão.
const TILE_CAPTION_H: f32 = 16.0;
/// Raio do canto do cartão (no tom dos campos/diálogos do kit).
const TILE_CORNER: u8 = 8;
/// Lado mínimo da célula do tile: cartão + a folga dos dois lados.
const TILE_CELL: f32 = TILE_SIDE + TILE_MARGIN * 2.0;
/// Altura de uma linha da grade: célula + legenda (o `show_rows` virtualiza
/// por linha, então ela tem que ser FIXA).
const TILE_ROW_H: f32 = TILE_CELL + TILE_CAPTION_H;
/// Lado máximo da miniatura decodificada — `thumbnail` e não a foto inteira.
const THUMB_MAX_SIDE: u32 = 256;
/// Teto do cache de miniaturas (~300 × 256² RGBA ≈ 75 MB: RAM/VRAM têm dono).
const THUMB_CACHE_CAP: usize = 300;
/// Largura do campo de busca (o resto da linha do breadcrumb é dos pills).
const SEARCH_W: f32 = 190.0;
/// Tamanho da tag "video" dentro do tile (não há frame sem ffmpeg).
const VIDEO_TAG_SIZE: f32 = 12.0;
/// Tamanho do spinner de "miniatura decodificando" dentro do tile.
const SPINNER_SIZE: f32 = 18.0;

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

/// Abre a foto numa INSTÂNCIA nova do próprio app: cada foto vira sua janela,
/// que é o mesmo caminho do "Abrir com" (o app já nasce abrindo o argv[1]).
fn open_in_new_window(path: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("localizar o executável: {e}"))?;
    std::process::Command::new(exe).arg(path).spawn().map(|_| ()).map_err(|e| format!("abrir nova janela: {e}"))
}

/// Janela em que o app está. A galeria é a HOME (boot sem arquivo); o viewer é
/// a foto (boot com arquivo ou duplo-clique na grade). Trocar de modo NÃO
/// destrói estado: `idx`/`tex`/`zoom` continuam lá e voltar retoma a foto.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Viewer,
    Gallery,
}

/// Diálogo aberto POR CIMA da galeria (um por vez — `None` = nenhum). Só
/// desenha e devolve a intenção: quem executa a operação é o app, depois do
/// desenho (durante ele o diálogo está emprestado).
enum Dialog {
    None,
    /// Nome novo do único arquivo selecionado.
    Rename {
        field: String,
    },
    /// Escolha da pasta de destino da seleção (subpastas + o pai).
    MoveTo,
    /// Confirmação da exclusão — que vai pra LIXEIRA, nunca pra definitiva.
    ConfirmDelete,
    /// Nome da pasta nova dentro da pasta corrente.
    NewFolder {
        field: String,
    },
}

/// Ação pedida pela linha de botões da galeria. Aplicada depois do desenho
/// da linha (durante ela o `self` está emprestado).
enum GalleryAction {
    NewFolder,
    Undo,
    Rename,
    MoveTo,
    Delete,
}

/// Botão de um diálogo. `Move` carrega o destino clicado na lista, que é o
/// único que precisa de dado além do "ok/cancelar".
enum DialogDone {
    Confirm,
    Cancel,
    Move(PathBuf),
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
    /// Miniatura da grade: decodifica pequeno (`thumbnail`) e devolve a
    /// textura. Muitos em paralelo — daí o canal fixo, ver `thumb_tx`.
    Thumb(PathBuf),
}

enum Done {
    List(Result<img::DirListing, String>),
    Decode {
        path: PathBuf,
        result: Result<(egui::ColorImage, Option<img::ImageInfo>), String>,
    },
    Exif(Vec<(String, String)>),
    Export(Result<PathBuf, String>),
    /// `image: None` = falhou (arquivo corrompido, formato sem decoder). Não
    /// é fatal: o tile cai no fallback (card + nome) em vez de pedir de novo.
    Thumb {
        path: PathBuf,
        image: Option<egui::ColorImage>,
    },
}

fn spawn_job(job: Job, tx: Sender<Done>) {
    std::thread::spawn(move || match job {
        Job::List(dir) => {
            let _ = tx.send(Done::List(img::list_entries(&dir)));
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
        Job::Thumb(path) => {
            // Erro AQUI não vira status: a grade tem centenas de tiles e um
            // arquivo podre não pode encher a barra de ⚠ — manda `None`.
            let thumb = image::open(&path).ok().map(|im| {
                let rgba = im.thumbnail(THUMB_MAX_SIDE, THUMB_MAX_SIDE).to_rgba8();
                egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
            });
            let _ = tx.send(Done::Thumb { path, image: thumb });
        }
    });
}

struct ImageApp {
    cfg: Config,
    dir: String,
    files: Vec<PathBuf>,
    /// Subpastas diretas da pasta corrente — as chips de navegação da
    /// galeria (o viewer só usa `files`).
    dirs: Vec<PathBuf>,
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
    // ---- galeria ----
    mode: Mode,
    /// Seleção da grade (Ctrl+clique soma/remove). Independente do `idx` do
    /// viewer: navegar na galeria não mexe na foto aberta.
    selection: HashSet<PathBuf>,
    /// Último tile clicado/focado — âncora das setas e do Enter.
    focus: Option<PathBuf>,
    /// Filtro da busca (substring sem caixa no nome do arquivo).
    search: String,
    /// Pede o scroll-into-view do tile focado (só quando o foco veio do
    /// teclado; com o mouse a grade fica onde o usuário deixou).
    scroll_focus: bool,
    /// Colunas do último frame — ↑/↓ pulam uma linha inteira da grade.
    grid_cols: usize,
    /// Canal FIXO das miniaturas: diferente do `request()` (um channel por
    /// job, sobrescrito a cada pedido), a grade pede dezenas EM PARALELO e
    /// cada resultado tem que voltar pro seu tile.
    thumb_tx: Sender<Done>,
    thumb_rx: Receiver<Done>,
    /// Paths com decode de miniatura em voo — dedup (a grade redesenha a
    /// cada frame e pediria a mesma miniatura sem parar).
    pending: HashSet<PathBuf>,
    /// Miniaturas que FALHARAM (arquivo podre, formato sem decoder): o tile
    /// fica no fallback (card + nome) e não pede de novo — sem isto a grade
    /// dispararia uma thread por frame, pra sempre, no mesmo arquivo.
    thumb_failed: HashSet<PathBuf>,
    /// Miniaturas prontas, por path, e a ordem FIFO da evicção.
    thumbs: HashMap<PathBuf, egui::TextureHandle>,
    thumb_order: VecDeque<PathBuf>,
    /// Diálogo de arquivo aberto por cima da grade (renomear/mover/excluir/
    /// nova pasta).
    dialog: Dialog,
    /// Pede o foco do campo do diálogo no primeiro frame dele (o usuário
    /// veio do teclado — F2/Ctrl+Z — e digitar já tem que cair no campo).
    dialog_focus: bool,
    /// Desfazer da SESSÃO (mover/renomear). A lixeira é o desfazer do
    /// excluir — ele não entra aqui.
    undo: fsops::UndoLog,
    /// Feedback da última operação de arquivo. A re-listagem que a operação
    /// pede LIMPA o `status` quando chega; guardar aqui e reaplicar no dreno
    /// mantém o `✓ n` na tela em vez de piscar por um frame.
    op_status: Option<String>,
}

impl ImageApp {
    fn new(cfg: Config, initial_file: Option<PathBuf>, opened_with_file: bool) -> Self {
        // Canal das miniaturas criado UMA vez: o receptor mora no app e cada
        // job leva um clone do transmissor.
        let (thumb_tx, thumb_rx) = std::sync::mpsc::channel();
        Self {
            cfg,
            dir: String::new(),
            files: Vec::new(),
            dirs: Vec::new(),
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
            // Boot com arquivo = fluxo do viewer de sempre; sem arquivo o
            // app abre na galeria (que passa a ser a home).
            mode: if opened_with_file { Mode::Viewer } else { Mode::Gallery },
            selection: HashSet::new(),
            focus: None,
            search: String::new(),
            scroll_focus: false,
            grid_cols: 1,
            thumb_tx,
            thumb_rx,
            pending: HashSet::new(),
            thumb_failed: HashSet::new(),
            thumbs: HashMap::new(),
            thumb_order: VecDeque::new(),
            dialog: Dialog::None,
            dialog_focus: false,
            undo: fsops::UndoLog::default(),
            op_status: None,
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
        // `.get` e não indexação: a galeria pode trocar a lista por baixo do
        // viewer (navegou pra outra pasta), e um `idx` velho estouraria.
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

    /// Abre a pasta do rodapé (pill "abrir" e Enter no campo — o caminho
    /// passa pelo saneamento: colar do "Copiar como caminho" do Windows
    /// traz aspas em volta).
    fn open_dir(&mut self) {
        let dir = img::sanitize_path_input(&self.dir);
        if dir.is_empty() {
            return;
        }
        // Devolve o caminho limpo pro campo: o usuário vê o que abriu.
        self.dir = dir.clone();
        // Pasta nova = seleção/foco/busca da galeria não valem mais.
        self.reset_gallery_nav();
        // Pasta nova = o viewer também recomeça: sem o reset, o open_first do
        // dreno não dispara (idx ocupado) e o viewer ficaria no idx da pasta
        // anterior.
        self.idx = None;
        self.tex = None;
        self.exif = None;
        // Nenhum `✓`/`⚠` de operação anterior sobrevive a uma navegação.
        self.op_status = None;
        self.request(Job::List(dir));
    }

    /// Zera o que era "da pasta anterior" na galeria: seleção, foco e filtro.
    fn reset_gallery_nav(&mut self) {
        self.selection.clear();
        self.focus = None;
        self.search.clear();
        self.scroll_focus = false;
    }

    /// Entra na galeria pela pill do viewer. Re-lista a pasta corrente: o
    /// `list_entries` também traz as SUBPASTAS, que o scan do viewer não
    /// pedia. Sem pasta aberta a galeria fica no estado vazio.
    fn enter_gallery(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Gallery;
        self.reset_gallery_nav();
        // Nenhum `✓`/`⚠` de operação anterior sobrevive a uma navegação.
        self.op_status = None;
        if !self.dir.is_empty() {
            self.request(Job::List(self.dir.clone()));
        }
        ctx.request_repaint();
    }

    /// Sai da galeria pro viewer (Esc). O estado do viewer é preservado, mas
    /// a pasta pode ter mudado por baixo dele: `idx` fora da lista nova volta
    /// pra "sem foto" e textura de outro arquivo é redecodificada.
    fn leave_gallery(&mut self) {
        self.mode = Mode::Viewer;
        if self.idx.is_some_and(|i| i >= self.files.len()) {
            self.idx = None;
            self.tex = None;
            self.exif = None;
        }
        if let Some(path) = self.current().cloned() {
            let stale = match &self.tex {
                Some((t, _)) => t != &path,
                None => true,
            };
            // Só imagem: vídeo não decodifica (o painel central mostra o
            // placeholder do player) e re-pedir o Exif seria à toa.
            if stale && img::is_image(&path) {
                // O EXIF em tela era do arquivo anterior — melhor o spinner
                // do que metadado errado.
                self.exif = None;
                self.request_decode(Job::Decode(path.clone()));
                self.request(Job::Exif(path));
            }
        }
    }

    /// Navega pra outra pasta DENTRO da galeria (breadcrumb, chip ou campo):
    /// troca a pasta corrente, limpa seleção/foco/busca e re-lista.
    fn navigate_dir(&mut self, dir: &Path, ctx: &egui::Context) {
        // Nenhum `✓`/`⚠` de operação anterior sobrevive a uma navegação.
        self.op_status = None;
        self.dir = dir.to_string_lossy().into_owned();
        self.reset_gallery_nav();
        self.request(Job::List(self.dir.clone()));
        ctx.request_repaint();
    }

    /// "Abrir" da galeria (duplo-clique/Enter): foto vira uma JANELA nova do
    /// app; vídeo vai pro player do sistema (a image crate não decodifica
    /// contêiner) e o status diz o que aconteceu, igual ao `goto` do viewer.
    fn open_item(&mut self, path: &Path) {
        if img::is_video(path) {
            self.status = match open_path(path) {
                Ok(()) => format!("▶ {}", i18n::t(self.cfg.lang, Key::VideoHint)),
                Err(e) => format!("⚠ {e}"),
            };
            return;
        }
        if let Err(e) = open_in_new_window(path) {
            self.status = format!("⚠ {e}");
        }
    }

    /// Pede a miniatura de `path` UMA vez — o `pending` deduplica (a grade
    /// redesenha a cada frame e pediria a mesma em loop).
    fn request_thumb(&mut self, path: &Path) {
        if self.thumbs.contains_key(path) || self.pending.contains(path) || self.thumb_failed.contains(path) {
            return;
        }
        let p = path.to_path_buf();
        self.pending.insert(p.clone());
        spawn_job(Job::Thumb(p), self.thumb_tx.clone());
    }

    /// Drena o canal fixo das miniaturas: cada resultado sai do `pending` e,
    /// se decodificou, vira textura no cache FIFO (com teto — RAM/VRAM têm
    /// dono). Falha só deixa o tile no fallback (card + nome) e entra no
    /// `thumb_failed`, pra não ser pedida de novo.
    fn drain_thumbs(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.thumb_rx.try_recv() {
            if let Done::Thumb { path, image } = done {
                self.pending.remove(&path);
                let Some(image) = image else {
                    self.thumb_failed.insert(path);
                    continue;
                };
                let tex = ctx.load_texture(format!("thumb:{path:?}"), image, egui::TextureOptions::LINEAR);
                self.thumbs.insert(path.clone(), tex);
                self.thumb_order.push_back(path);
                while self.thumb_order.len() > THUMB_CACHE_CAP {
                    if let Some(old) = self.thumb_order.pop_front() {
                        self.thumbs.remove(&old);
                    }
                }
            }
        }
    }

    /// Índices de `self.files` que passam pela busca (substring sem caixa no
    /// nome do arquivo). Vazio de busca = a pasta inteira, na ordem do viewer.
    fn visible_files(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return (0..self.files.len()).collect();
        }
        self.files
            .iter()
            .enumerate()
            .filter(|(_, p)| p.file_name().map(|n| n.to_string_lossy().to_lowercase().contains(&needle)).unwrap_or(false))
            .map(|(i, _)| i)
            .collect()
    }

    /// Quantas colunas de tile cabem em `width` (mínimo 1). A barra de
    /// rolagem vertical é descontada SEMPRE: com o número de colunas estável,
    /// a grade não repagina quando a barra aparece no meio de uma pasta.
    fn grid_columns(ui: &egui::Ui, width: f32) -> usize {
        let spacing = ui.spacing().item_spacing.x;
        let usable = (width - ui.spacing().scroll.allocated_width()).max(TILE_CELL);
        (((usable + spacing) / (TILE_CELL + spacing)).floor() as usize).max(1)
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

/// Intenção de um clique num tile — devolvida DEPOIS de desenhar, porque
/// durante o desenho o tile está emprestando o app.
enum TileAction {
    None,
    Select { ctrl: bool },
    Open,
}

/// Rótulo curto de uma op do desfazer (o hover do botão diz o que volta):
/// `antigo → novo`, com o caminho de verdade nas duas pontas.
fn op_label(op: &fsops::Op) -> String {
    let (from, to) = match op {
        fsops::Op::Move { from, to } | fsops::Op::Rename { from, to } => (from, to),
    };
    format!("{} → {}", from.display(), to.display())
}

/// Texto que o USUÁRIO lê para um erro de arquivo. O `fsops` devolve o
/// variante estruturado; a tradução nasce aqui, na borda (o `Display` de lá
/// continua em português — é o que os testes e o log conhecem).
fn fs_err(lang: Lang, e: &fsops::FsError) -> String {
    use fsops::FsError as E;
    match e {
        E::EmptyName => i18n::t(lang, Key::FsErrEmptyName).to_string(),
        E::Separator => i18n::t(lang, Key::FsErrSeparator).to_string(),
        E::DotDot => i18n::t(lang, Key::FsErrDotDot).to_string(),
        E::NoParent => i18n::t(lang, Key::FsErrNoParent).to_string(),
        E::NoName => i18n::t(lang, Key::FsErrNoName).to_string(),
        // O nome colide no fim da frase (fonte termina em ": ").
        E::Exists(name) => format!("{}{name}", i18n::t(lang, Key::FsErrExists)),
        E::CrossVolume => i18n::t(lang, Key::FsErrCrossVolume).to_string(),
        E::RenameIo(err) => format!("{}: {err}", i18n::t(lang, Key::FsErrRename)),
        E::MoveIo(err) => format!("{}: {err}", i18n::t(lang, Key::FsErrMove)),
        E::CreateIo(err) => format!("{}: {err}", i18n::t(lang, Key::FsErrCreate)),
        E::UndoIo(err) => format!("{}: {err}", i18n::t(lang, Key::FsErrUndo)),
        #[cfg(windows)]
        E::TrashCode(n) => format!("{}: código {n}", i18n::t(lang, Key::FsErrTrash)),
        #[cfg(windows)]
        E::Aborted => i18n::t(lang, Key::FsErrCancelled).to_string(),
        #[cfg(not(windows))]
        E::TrashOut(msg) => format!("{}: {msg}", i18n::t(lang, Key::FsErrTrash)),
        #[cfg(not(windows))]
        E::NoGio => i18n::t(lang, Key::FsErrNoGio).to_string(),
        #[cfg(not(windows))]
        E::TrashIo(err) => format!("{}: {err}", i18n::t(lang, Key::FsErrTrash)),
    }
}

impl ImageApp {
    /// Abre um diálogo e pede o foco do campo dele no primeiro frame (quem
    /// entrou por F2/Ctrl+Z quer digitar, não clicar no campo).
    fn set_dialog(&mut self, dialog: Dialog) {
        self.dialog = dialog;
        self.dialog_focus = true;
    }

    /// Renomear: o campo já vem com o nome ATUAL — renomear é corrigir, não
    /// digitar do zero.
    fn open_rename(&mut self) {
        let Some(path) = self.selection.iter().next() else {
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.set_dialog(Dialog::Rename { field: name });
    }

    /// Renomeia o ÚNICO arquivo da seleção (o diálogo só abre assim).
    fn apply_rename(&mut self, new_name: &str) {
        let Some(path) = self.selection.iter().next().cloned() else {
            return;
        };
        match fsops::rename_file(&path, new_name) {
            Ok(dst) => {
                // Mesmo nome = no-op: não vira entrada de desfazer.
                if dst != path {
                    self.undo.push(fsops::Op::Rename { from: path, to: dst });
                }
                self.finish_ops(1, Vec::new());
            }
            Err(e) => {
                self.status = format!("⚠ {}", fs_err(self.cfg.lang, &e));
                // Erro de nome/colisão é do CAMPO, não do usuário: reabre o
                // diálogo com o que foi digitado pra ele corrigir, não
                // redigitar.
                self.set_dialog(Dialog::Rename { field: new_name.to_string() });
            }
        }
    }

    /// Move TODA a seleção pra `dst_dir` (uma op de desfazer por arquivo:
    /// cada Ctrl+Z devolve um, na ordem inversa).
    fn apply_move(&mut self, dst_dir: &Path) {
        let lang = self.cfg.lang;
        let mut ok = 0;
        let mut errs: Vec<String> = Vec::new();
        let mut failed: Vec<PathBuf> = Vec::new();
        for src in std::mem::take(&mut self.selection) {
            match fsops::move_file(&src, dst_dir) {
                Ok(dst) => {
                    ok += 1;
                    if dst != src {
                        self.undo.push(fsops::Op::Move { from: src, to: dst });
                    }
                }
                Err(e) => {
                    errs.push(fs_err(lang, &e));
                    failed.push(src);
                }
            }
        }
        self.finish_ops(ok, errs);
        // O `finish_ops` limpa a seleção; quem FALHOU volta pra ela — o
        // usuário corrige o motivo e tenta de novo sem reselecionar tudo.
        self.restore_selection(failed);
    }

    /// Exclui a seleção pra LIXEIRA do sistema (recuperável lá — o desfazer
    /// do excluir é a própria lixeira, por isso nada entra no `undo`).
    fn apply_delete(&mut self) {
        let lang = self.cfg.lang;
        let mut ok = 0;
        let mut errs: Vec<String> = Vec::new();
        let mut failed: Vec<PathBuf> = Vec::new();
        for path in std::mem::take(&mut self.selection) {
            match fsops::delete_to_trash(&path) {
                Ok(()) => ok += 1,
                Err(e) => {
                    errs.push(fs_err(lang, &e));
                    failed.push(path);
                }
            }
        }
        self.finish_ops(ok, errs);
        self.restore_selection(failed);
    }

    /// Devolve à seleção/foco os paths que a operação NÃO conseguiu mexer.
    fn restore_selection(&mut self, failed: Vec<PathBuf>) {
        if failed.is_empty() {
            return;
        }
        self.focus = failed.last().cloned();
        self.selection.extend(failed);
    }

    /// Cria a pasta no nome digitado, dentro da pasta corrente.
    fn apply_new_folder(&mut self, name: &str) {
        match fsops::create_dir(Path::new(&self.dir), name) {
            Ok(_) => self.finish_ops(1, Vec::new()),
            Err(e) => self.status = format!("⚠ {}", fs_err(self.cfg.lang, &e)),
        }
    }

    /// Ctrl+Z / pill Desfazer: reverte o topo do log (mover ou renomear).
    fn undo_last(&mut self) {
        match self.undo.undo() {
            Ok(Some(_)) => self.finish_ops(1, Vec::new()),
            Ok(None) => {}
            Err(e) => self.status = format!("⚠ {}", fs_err(self.cfg.lang, &e)),
        }
    }

    /// Fecha um lote de operações: status (`✓ n` ou o primeiro erro) e
    /// re-lista a pasta — os paths da grade mudaram de nome, de pasta ou
    /// deixaram de existir. Seleção e foco saem junto: apontariam pra paths
    /// que não estão mais lá.
    fn finish_ops(&mut self, ok: usize, errs: Vec<String>) {
        let lang = self.cfg.lang;
        self.selection.clear();
        self.focus = None;
        let msg = match errs.first() {
            Some(e) => format!("⚠ {e}"),
            None if ok > 0 => format!("✓ {}", i18n::t_items(lang, ok)),
            None => String::new(),
        };
        // A listagem que vem logo abaixo zera o `status` (dreno do `ui()`):
        // o `op_status` faz o feedback sobreviver a ela.
        self.op_status = Some(msg.clone());
        self.status = msg;
        if !self.dir.is_empty() {
            self.request(Job::List(self.dir.clone()));
        }
    }

    /// Diálogo de arquivo da galeria (renomear/mover/excluir/nova pasta):
    /// janela centrada com a moldura do kit. O diálogo sai do `self` antes do
    /// desenho — dentro da janela o campo precisa de `&mut` e o resto do app
    /// fica livre pros dados que ela mostra (`dirs`, `selection`).
    fn gallery_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = std::mem::replace(&mut self.dialog, Dialog::None);
        let (title, width) = match &dialog {
            Dialog::None => return,
            Dialog::Rename { .. } => (i18n::t(self.cfg.lang, Key::Rename), 340.0),
            Dialog::MoveTo => (i18n::t(self.cfg.lang, Key::MoveTo), 340.0),
            Dialog::ConfirmDelete => (i18n::t(self.cfg.lang, Key::Delete), 360.0),
            Dialog::NewFolder { .. } => (i18n::t(self.cfg.lang, Key::NewFolder), 340.0),
        };
        let lang = self.cfg.lang;
        // Só o campo de pasta nova tem placeholder (o de renomear já nasce
        // preenchido com o nome atual).
        let hint = match &dialog {
            Dialog::NewFolder { .. } => i18n::t(lang, Key::FolderName),
            _ => "",
        };
        let mut done: Option<DialogDone> = None;
        let focus_field = self.dialog_focus;
        self.dialog_focus = false;
        egui::Window::new(title)
            .id(egui::Id::unique("galeria-dialogo"))
            .frame(modal_frame(ctx))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(width)
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(width);
                match &mut dialog {
                    Dialog::None => {}
                    Dialog::Rename { field } | Dialog::NewFolder { field } => {
                        let resp = ui.add(line(field).hint_text(hint).desired_width(f32::INFINITY));
                        if focus_field {
                            resp.request_focus();
                        }
                        // Enter confirma: o TextEdit perde o foco no Enter
                        // (o mesmo padrão do campo de pasta do rodapé).
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            done = Some(DialogDone::Confirm);
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if pill(ui, i18n::t(lang, Key::Confirm), true).clicked() {
                                done = Some(DialogDone::Confirm);
                            }
                            if pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                                done = Some(DialogDone::Cancel);
                            }
                        });
                    }
                    Dialog::MoveTo => {
                        // Destinos possíveis: o pai (`..`) e as subpastas da
                        // pasta corrente. Lista com rolagem: pasta com muitas
                        // filhas não pode empurrar o diálogo pra fora da tela.
                        let parent = Path::new(&self.dir).parent().map(|p| {
                            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
                            (format!(".. {name}"), p.to_path_buf())
                        });
                        egui::ScrollArea::vertical().id_salt("mover-destinos").max_height(240.0).show(ui, |ui| {
                            if let Some((label, path)) = &parent {
                                if pill(ui, label, false).clicked() {
                                    done = Some(DialogDone::Move(path.clone()));
                                }
                            }
                            for d in &self.dirs {
                                let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| d.display().to_string());
                                if pill(ui, &format!("📁 {name}"), false).clicked() {
                                    done = Some(DialogDone::Move(d.clone()));
                                }
                            }
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                                done = Some(DialogDone::Cancel);
                            }
                            ui.label(egui::RichText::new(i18n::t_items(lang, self.selection.len())).small().weak());
                        });
                    }
                    Dialog::ConfirmDelete => {
                        ui.label(i18n::t_items(lang, self.selection.len()));
                        ui.label(egui::RichText::new(i18n::t(lang, Key::DeleteToTrashHint)).small().weak());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            // Excluir manda pra lixeira: o Confirmar ganha o
                            // sinal de perigo (o mesmo vermelho dos destrutivos
                            // do kit).
                            if pill_danger(ui, i18n::t(lang, Key::Confirm)).clicked() {
                                done = Some(DialogDone::Confirm);
                            }
                            if pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                                done = Some(DialogDone::Cancel);
                            }
                        });
                    }
                }
            });
        // `dialog` volta pro app só quando o usuário NÃO fechou: aí o próximo
        // frame desenha de novo (com o campo como ele está).
        match done {
            None => self.dialog = dialog,
            Some(DialogDone::Cancel) => {}
            Some(DialogDone::Confirm) => match dialog {
                Dialog::Rename { field } => self.apply_rename(&field),
                Dialog::NewFolder { field } => self.apply_new_folder(&field),
                Dialog::ConfirmDelete => self.apply_delete(),
                // Mover confirma por clique no destino, não pelo "ok".
                _ => {}
            },
            Some(DialogDone::Move(dst)) => self.apply_move(&dst),
        }
    }

    /// Galeria (a home do app): barra de topo, breadcrumb + busca, chips de
    /// subpastas, grade virtualizada e rodapé de status. Vive no painel
    /// central; o viewer segue montado por baixo, com todo o estado.
    fn gallery_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, kb_free: bool) {
        // A lista FILTRADA manda em tudo (contagem, teclado e grade).
        let visible = self.visible_files();
        let cols = Self::grid_columns(ui, ui.available_width());
        self.grid_cols = cols;
        if kb_free {
            self.gallery_keys(ctx, &visible);
        }

        ui.horizontal(|ui| {
            ui.strong("Local Image");
            // Com List no ar a contagem é a da listagem VELHA — "working…"
            // em vez de um "0 items" enganoso.
            let count =
                if self.rx.is_some() { i18n::t(self.cfg.lang, Key::Working).to_string() } else { i18n::t_items(self.cfg.lang, visible.len()) };
            ui.label(egui::RichText::new(count).small().weak());
            // Direita→esquerda: configurações e o check de update, os mesmos
            // controles da toolbar do viewer.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (changed, _) = lab_ui::settings_ui(ui, &mut self.cfg, false);
                if changed {
                    theme::apply(ctx, self.cfg.theme);
                    let _ = config::save(APP_ID, &self.cfg);
                }
                if ghost(ui, i18n::t(self.cfg.lang, Key::CheckUpdates)).clicked() {
                    self.check_updates(ctx);
                }
            });
        });

        // Caminho inteiro em pills (o último é a pasta corrente, texto normal
        // — clicar nele não faria nada). A busca fica na ponta direita.
        let trail: Vec<(PathBuf, String)> = img::ancestors(Path::new(&self.dir))
            .into_iter()
            .map(|p| {
                // Raiz (`C:\`, `/`) não tem `file_name`: mostra o path dela.
                let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
                (p, label)
            })
            .collect();
        let mut nav: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if self.dir.is_empty() {
                // Sem pasta aberta: o campo do rodapé vem pra cá (é aqui que
                // o usuário escolhe a primeira pasta da galeria). Espelho do
                // rodapé do viewer: direita→esquerda, o pill entra PRIMEIRO
                // (canto direito) e o campo preenche o resto — na ordem
                // natural o `desired_width(INFINITY)` empurrava o "abrir".
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill(ui, i18n::t(self.cfg.lang, Key::Open), true).clicked() {
                        self.open_dir();
                    }
                    // `align`: o caminho é conteúdo LTR (mesmo motivo do rodapé).
                    let resp = ui.add(line(&mut self.dir).hint_text(r"C:\...\fotos").align(egui::Align2::LEFT_CENTER).desired_width(f32::INFINITY));
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.open_dir();
                    }
                });
            } else {
                let last = trail.len().saturating_sub(1);
                for (i, (path, label)) in trail.iter().enumerate() {
                    if i > 0 {
                        ui.label(egui::RichText::new("›").weak());
                    }
                    if i == last {
                        ui.label(label);
                    } else if pill(ui, label, false).clicked() {
                        nav = Some(path.clone());
                    }
                }
                // O layout da direita pra esquerda ocupa o que sobrou da linha
                // e encosta o campo na borda (mesmo truque do rodapé).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        line(&mut self.search)
                            .hint_text(i18n::t(self.cfg.lang, Key::Search))
                            .align(egui::Align2::LEFT_CENTER)
                            .desired_width(SEARCH_W),
                    );
                });
            }
        });

        if !self.dirs.is_empty() {
            // Chips das subpastas: rolagem horizontal (pasta com muitas filhas
            // não pode empurrar a grade pra fora da janela).
            egui::ScrollArea::horizontal().id_salt("subpastas").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for d in &self.dirs {
                        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| d.display().to_string());
                        if pill(ui, &format!("📁 {name}"), false).clicked() {
                            nav = Some(d.clone());
                        }
                    }
                });
            });
        }
        // A navegação só é aplicada AQUI: durante a montagem da linha o
        // `self.dirs` está emprestado (e a lista só troca no próximo frame).
        if let Some(target) = nav {
            self.navigate_dir(&target, ctx);
        }

        // Linha de AÇÕES da pasta: nova pasta e desfazer valem sempre (têm
        // pasta corrente); renomear/mover/excluir só com seleção — renomear
        // exige UMA, porque o diálogo pede um nome só.
        if !self.dir.is_empty() {
            let lang = self.cfg.lang;
            let mut action: Option<GalleryAction> = None;
            ui.horizontal(|ui| {
                if pill(ui, i18n::t(lang, Key::NewFolder), false).clicked() {
                    action = Some(GalleryAction::NewFolder);
                }
                // Desfazer nasce desabilitado (nada pra desfazer) e o hover
                // diz O QUE seria desfeito.
                let undo_slot = ui.add_enabled_ui(!self.undo.is_empty(), |ui| pill(ui, i18n::t(lang, Key::Undo), false));
                let undo_resp = match self.undo.peek() {
                    Some(op) => undo_slot.inner.on_hover_text(format!("Ctrl+Z — {}", op_label(op))),
                    None => undo_slot.inner.on_hover_text("Ctrl+Z"),
                };
                if undo_resp.clicked() {
                    action = Some(GalleryAction::Undo);
                }
                if self.selection.len() == 1 && pill(ui, i18n::t(lang, Key::Rename), false).on_hover_text("F2").clicked() {
                    action = Some(GalleryAction::Rename);
                }
                if !self.selection.is_empty() {
                    if pill(ui, i18n::t(lang, Key::MoveTo), false).clicked() {
                        action = Some(GalleryAction::MoveTo);
                    }
                    if pill(ui, i18n::t(lang, Key::Delete), false).on_hover_text("Delete").clicked() {
                        action = Some(GalleryAction::Delete);
                    }
                }
            });
            match action {
                Some(GalleryAction::NewFolder) => self.set_dialog(Dialog::NewFolder { field: String::new() }),
                Some(GalleryAction::Undo) => self.undo_last(),
                Some(GalleryAction::Rename) => self.open_rename(),
                Some(GalleryAction::MoveTo) => self.set_dialog(Dialog::MoveTo),
                Some(GalleryAction::Delete) => self.set_dialog(Dialog::ConfirmDelete),
                None => {}
            }
        }

        // Rodapé fino com o status (mesmo papel do rodapé do viewer). A
        // altura é medida no estilo e descontada da grade: o `ScrollArea`
        // come todo o espaço que sobra.
        let footer_h = egui::TextStyle::Small.resolve(ui.style()).size + ui.spacing().item_spacing.y;
        let grid_h = (ui.available_height() - footer_h).max(0.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), grid_h), egui::Layout::top_down(egui::Align::Min), |ui| {
            // Hierarquia: List no ar → spinner; pasta nenhuma → convite pra
            // abrir; pasta aberta sem mídia (ou busca sem resultado) → a
            // mesma frase (o filtro está à vista no campo).
            if self.rx.is_some() {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                return;
            }
            if visible.is_empty() {
                let key = if self.dir.is_empty() { Key::PickFolder } else { Key::NoMedia };
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(i18n::t(self.cfg.lang, key)).weak());
                });
                return;
            }
            self.gallery_grid(ui, &visible, cols);
        });
        ui.label(egui::RichText::new(&self.status).small().weak());

        // Scrim do diálogo, desenhado por ÚLTIMO na grade: sem ele o clique
        // fora da janela caía num tile e trocava a seleção com o "Excluir"
        // aberto. O diálogo é uma Window (layer acima), então continua
        // clicável; aqui só sobra o clique FORA = cancelar. Preto a 100/255 é
        // constante deliberada (véu do modal, não cor de tema).
        if !matches!(self.dialog, Dialog::None) {
            let resp = ui.allocate_rect(ui.max_rect(), egui::Sense::click());
            ui.painter().rect_filled(ui.max_rect(), 0, egui::Color32::from_black_alpha(100));
            if resp.clicked() {
                self.dialog = Dialog::None;
            }
        }
    }

    /// Teclado da galeria (só fora de campo de texto): setas movem o foco na
    /// lista FILTRADA — ←/→ um tile, ↑/↓ uma linha — e a seleção fica só com
    /// ele; Enter abre o focado. Delete/F2/Ctrl+Z são das AÇÕES de arquivo.
    fn gallery_keys(&mut self, ctx: &egui::Context, visible: &[usize]) {
        // Diálogo aberto manda no teclado: sem esta guarda as teclas de ação
        // agiriam por baixo da janela.
        if !matches!(self.dialog, Dialog::None) {
            return;
        }
        // `consume_key` e não `key_pressed`: o egui pode rodar DOIS passes no
        // mesmo frame (layout com `request_discard`) e o `App::ui` roda em
        // cada um — sem consumir, uma seta andaria dois tiles.
        let apertada = |key: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        // Ações de arquivo valem ANTES do corte da lista vazia: a seleção
        // pode ter sobrado de um filtro de busca, e excluir/mover não olham
        // pro que está visível.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
            self.undo_last();
            return;
        }
        if apertada(egui::Key::Delete) && !self.selection.is_empty() {
            self.set_dialog(Dialog::ConfirmDelete);
            return;
        }
        if apertada(egui::Key::F2) && self.selection.len() == 1 {
            self.open_rename();
            return;
        }
        if visible.is_empty() {
            return;
        }
        let (left, right, up, down, enter) = (
            apertada(egui::Key::ArrowLeft),
            apertada(egui::Key::ArrowRight),
            apertada(egui::Key::ArrowUp),
            apertada(egui::Key::ArrowDown),
            apertada(egui::Key::Enter),
        );
        let pos = self.focus.as_ref().and_then(|f| visible.iter().position(|&i| &self.files[i] == f));
        let last = visible.len() - 1;
        let cols = self.grid_cols.max(1);
        // Bordas: clamp — as setas não dão a volta na grade.
        let target = if left {
            Some(pos.map_or(0, |c| c.saturating_sub(1)))
        } else if right {
            Some(pos.map_or(0, |c| (c + 1).min(last)))
        } else if up {
            Some(pos.map_or(0, |c| c.saturating_sub(cols)))
        } else if down {
            Some(pos.map_or(0, |c| (c + cols).min(last)))
        } else {
            None
        };
        if let Some(t) = target {
            let path = self.files[visible[t]].clone();
            self.focus = Some(path.clone());
            self.selection.clear();
            self.selection.insert(path);
            // Quem anda com o teclado não está olhando o mouse: a grade
            // acompanha o foco (uma vez só, no tile dele).
            self.scroll_focus = true;
            ctx.request_repaint();
        } else if enter {
            if let Some(path) = self.focus.clone() {
                self.open_item(&path);
            }
        }
    }

    /// Grade virtualizada: `show_rows` desenha só as linhas visíveis, então
    /// uma pasta de milhares de fotos custa o mesmo que uma de 30 — e só os
    /// tiles que existem neste frame pedem miniatura (`request_thumb`).
    fn gallery_grid(&mut self, ui: &mut egui::Ui, visible: &[usize], cols: usize) {
        // `cols` é no mínimo 1 (garantido por `grid_columns`).
        // `div_ceil` e não a conta manual: o clippy do CI (-D warnings)
        // barra `manual_div_ceil`.
        let rows = visible.len().div_ceil(cols);
        egui::ScrollArea::vertical().id_salt("grade").auto_shrink([false, false]).show_rows(ui, TILE_ROW_H, rows, |ui, range| {
            // Foco vindo do teclado: rola até a LINHA dele. O tile pode nem
            // ser desenhado neste frame (fora da janela visível), então o
            // alvo é a linha — não o tile.
            if self.scroll_focus {
                if let Some(pos) = self.focus.as_ref().and_then(|f| visible.iter().position(|&i| &self.files[i] == f)) {
                    let row_h = TILE_ROW_H + ui.spacing().item_spacing.y;
                    let top = ui.max_rect().top() + ((pos / cols) as f32 - range.start as f32) * row_h;
                    let target = egui::Rect::from_min_size(egui::pos2(ui.max_rect().left(), top), egui::vec2(ui.available_width(), TILE_ROW_H));
                    ui.scroll_to_rect(target, Some(egui::Align::Center));
                    self.scroll_focus = false;
                }
            }
            for row in range {
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        let Some(&i) = visible.get(row * cols + col) else { break };
                        // `path` sai daqui por clone: `self.tile` precisa de
                        // `&mut self` e a lista não pode ficar emprestada.
                        let path = self.files[i].clone();
                        let selected = self.selection.contains(&path);
                        let focused = self.focus.as_ref() == Some(&path);
                        match self.tile(ui, &path, selected, focused) {
                            TileAction::None => {}
                            TileAction::Select { ctrl } => {
                                // Ctrl soma/remove; clique puro troca a seleção.
                                let removed = ctrl && self.selection.remove(&path);
                                if !removed {
                                    if !ctrl {
                                        self.selection.clear();
                                    }
                                    self.selection.insert(path.clone());
                                }
                                self.focus = Some(path);
                            }
                            TileAction::Open => self.open_item(&path),
                        }
                    }
                });
            }
        });
    }

    /// Um tile da grade: cartão com a miniatura (ou com o nome, nos vídeos e
    /// nas miniaturas que falharam) + a legenda embaixo.
    fn tile(&mut self, ui: &mut egui::Ui, path: &Path, selected: bool, focused: bool) -> TileAction {
        let skin = Skin::of(ui);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_CELL, TILE_ROW_H), egui::Sense::click());
        let card = egui::Rect::from_min_size(rect.min + egui::vec2(TILE_MARGIN, TILE_MARGIN), egui::vec2(TILE_SIDE, TILE_SIDE));
        // Cópia do painter: desenhar não pode prender o `ui` (o spinner e a
        // legenda ainda precisam dele emprestado de forma mutável).
        let painter = ui.painter().clone();
        painter.rect_filled(card, egui::CornerRadius::same(TILE_CORNER), skin.card);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // O tile é o widget clicável (o desenho é à mão): leitor de tela
        // precisa do nome do arquivo. Não há `Role::ImageButton` no fork
        // (Role = accesskit::Role) — `Button` é o papel mais próximo.
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));

        if img::is_video(path) {
            // Sem ffmpeg não há frame do vídeo: o cartão fica com a tag e o
            // nome embaixo — o duplo-clique manda pro player do sistema.
            painter.text(
                card.center(),
                egui::Align2::CENTER_CENTER,
                i18n::t(self.cfg.lang, Key::VideoTile),
                egui::FontId::proportional(VIDEO_TAG_SIZE),
                skin.muted,
            );
        } else if let Some(tex) = self.thumbs.get(path) {
            // "Contain": a miniatura cabe sem esticar (o decode limitou o lado
            // maior, mas o cartão não tem a mesma proporção da foto).
            let s = tex.size_vec2();
            let k = (card.width() / s.x).min(card.height() / s.y);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(tex.id(), egui::Rect::from_center_size(card.center(), s * k), uv, egui::Color32::WHITE);
        } else {
            // Sem textura ainda: ou o decode está no ar (spinner), ou falhou —
            // aí fica o cartão vazio (o nome embaixo identifica o arquivo) e
            // o `thumb_failed` impede novo pedido.
            if self.pending.contains(path) {
                ui.put(card, egui::Spinner::new().size(SPINNER_SIZE));
            }
            self.request_thumb(path);
        }

        // Seleção = contorno do accent (2px). Foco sem seleção só acontece
        // com Ctrl+clique removendo o item: contorno fraco, mais discreto.
        // Hover usa a borda do tema (a mesma lavagem sutil dos pills).
        let ring = if selected {
            Some(egui::Stroke::new(2.0, skin.accent))
        } else if focused {
            Some(egui::Stroke::new(1.0, skin.muted))
        } else if resp.hovered() {
            Some(egui::Stroke::new(1.0, skin.border))
        } else {
            None
        };
        if let Some(stroke) = ring {
            painter.rect_stroke(card, egui::CornerRadius::same(TILE_CORNER), stroke, egui::StrokeKind::Inside);
        }

        // Legenda: Small/fraca e truncada com "…" (nome longo não vaza do tile).
        let cap = egui::Rect::from_min_max(egui::pos2(rect.left(), card.bottom() + TILE_MARGIN), egui::pos2(rect.right(), rect.bottom()));
        ui.put(cap, egui::Label::new(egui::RichText::new(&name).small().weak()).truncate());

        let resp = resp.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand);
        // Duplo-clique ANTES do clique: no egui o duplo também conta o clique
        // simples do primeiro toque.
        if resp.double_clicked() {
            TileAction::Open
        } else if resp.clicked() {
            TileAction::Select { ctrl: ui.input(|i| i.modifiers.ctrl || i.modifiers.command) }
        } else {
            TileAction::None
        }
    }
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
        // Atalhos do VIEWER (zoom/giro/tela cheia/EXIF/setas) só valem no
        // modo dele: na galeria as mesmas teclas são da grade.
        let viewer = self.mode == Mode::Viewer;

        // R = redimensiona a janela pra imagem ATUAL (abriu com uma,
        // navegou pras setas — a janela acompanha a nova). Dims já com a
        // rotação aplicada (90° troca os eixos).
        if viewer && kb_free && ctx.input(|i| i.key_pressed(egui::Key::R)) {
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
        if viewer && kb_free && self.tex.is_some() {
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
                    Done::List(Ok(listing)) => {
                        self.dirs = listing.dirs;
                        self.files = listing.files;
                        self.status.clear();
                        // Operação de arquivo que pediu esta listagem: o
                        // `✓`/`⚠` dela volta pro rodapé (o clear acima é só
                        // pra erro de navegação velho não ficar na tela).
                        if let Some(msg) = self.op_status.take() {
                            self.status = msg;
                        }
                        // A galeria NÃO abre a primeira foto (a home é a
                        // grade); só o fluxo do viewer — boot "Abrir com" —
                        // precisa dar o `goto`. `idx` ocupado = o viewer já
                        // está numa foto: um List atrasado da galeria não
                        // pode pular o usuário de volta pra foto 0.
                        if !self.files.is_empty() && self.mode == Mode::Viewer && self.idx.is_none() {
                            open_first = true;
                        }
                        // A lista pode ter mudado por baixo do viewer
                        // (operação de arquivo, navegação na galeria):
                        // revalida o índice e a textura como o
                        // `leave_gallery` faz — foto removida vira "sem
                        // foto" e textura de outro arquivo é redecodificada.
                        // O `goto` fica pra DEPOIS do laço: dentro dele o
                        // `self.rx` ainda está emprestado.
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
                                // Só no VIEWER: na galeria o `goto` dispararia
                                // decode (e, num vídeo, o player do sistema)
                                // por baixo da grade — o `leave_gallery` já
                                // revalida na volta.
                                if self.mode == Mode::Viewer {
                                    revalidate_goto = self.idx;
                                }
                            }
                        }
                    }
                    Done::List(Err(e)) => {
                        // O `✓`/`⚠` de uma operação anterior não sobrevive
                        // a uma listagem que falhou: o erro de agora é o que
                        // importa na tela.
                        self.op_status = None;
                        self.status = format!("⚠ {e}");
                    }
                    Done::Exif(entries) => self.exif = Some(entries),
                    Done::Export(Ok(dst)) => {
                        self.status = format!("✓ {}", dst.display());
                    }
                    Done::Export(Err(e)) => self.status = format!("⚠ {e}"),
                    _ => {}
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.rx = None;
            }
        }
        // Miniaturas: canal FIXO (o `request()` de um job por vez não serve —
        // a grade pede dezenas em paralelo e cada resultado tem que voltar pro
        // seu tile). Erro já vem como `None`: o tile cai no fallback.
        self.drain_thumbs(ctx);
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
        if self.rx.is_some() || self.decode_rx.is_some() || !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        // Atalhos de navegação (mesmos do oficial) — só no viewer: na galeria
        // as setas são da grade.
        if viewer {
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
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.mode == Mode::Gallery {
                // Esc com diálogo aberto fecha SÓ o diálogo (o mesmo Esc não
                // pode derrubar a galeria junto).
                if !matches!(self.dialog, Dialog::None) {
                    self.dialog = Dialog::None;
                    ctx.request_repaint();
                } else if self.idx.is_some() {
                    // Esc na galeria volta pra foto — mas só se HÁ foto pra
                    // voltar: quem abriu o app sem arquivo fica na home.
                    self.leave_gallery();
                    ctx.request_repaint();
                }
            } else if self.chrome && self.show_exif {
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
        if viewer && kb_free && ctx.input(|i| i.key_pressed(egui::Key::F)) {
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
        if viewer && kb_free && self.tex.is_some() && ctx.input(|i| i.key_pressed(egui::Key::C)) {
            self.center_pending = true;
        }
        if viewer && kb_free && ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
            self.refit();
        }
        // i = painel EXIF (mesma guarda de letra: digitar "i" no campo do
        // rodapé não pode abrir o painel).
        if viewer && kb_free && ctx.input(|i| i.key_pressed(egui::Key::I)) {
            self.show_exif = !self.show_exif;
        }

        // Interface: toolbar/rodapé só quando `chrome` — e só no viewer (a
        // galeria tem barra e rodapé próprios dentro do painel central).
        let chrome = self.chrome && self.mode == Mode::Viewer;
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
                        // Modo galeria: entra e re-lista a pasta corrente (a
                        // pill fica sempre no chrome, ao lado do export).
                        if pill(ui, i18n::t(self.cfg.lang, Key::Gallery), false).on_hover_text(i18n::t(self.cfg.lang, Key::GalleryHint)).clicked() {
                            self.enter_gallery(ctx);
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
            // A galeria é um modo inteiro do painel central; o viewer (abaixo)
            // só desenha no modo dele.
            if self.mode == Mode::Gallery {
                self.gallery_ui(ui, ctx, kb_free);
                return;
            }

            if self.files.is_empty() {
                // Sem pasta e sem interface → clique mostra a toolbar
                // (senão o usuário fica preso).
                let resp = ui
                    .centered_and_justified(|ui| {
                        ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::PickFolder)).weak());
                    })
                    .response;
                if resp.clicked() {
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

        // Painel EXIF é do viewer (a galeria tem a barra dela e o painel
        // flutuante cobriria a grade).
        if self.show_exif && viewer {
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

        // Diálogo de arquivo da galeria (renomear/mover/excluir/nova pasta):
        // janela centrada por cima da grade, só no modo galeria.
        if self.mode == Mode::Gallery {
            self.gallery_dialog(ctx);
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

        // Repaint de novo no FIM: as miniaturas são pedidas DURANTE o desenho
        // da grade (o check lá em cima roda antes) e o List das operações só
        // é drenado no próximo frame. Sem este segundo agendamento a grade
        // fica congelada até o mouse se mexer.
        if self.rx.is_some() || self.decode_rx.is_some() || !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("local-image-gal-{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Pasta com uma subpasta, uma foto de verdade (a miniatura decodifica),
    /// um vídeo (só a extensão importa: vídeo nunca é decodificado) e uma
    /// foto PODRE (a miniatura falha — tem que cair no fallback sem insistir).
    fn pasta_com_midia(tag: &str) -> PathBuf {
        let d = tmpdir(tag);
        std::fs::create_dir_all(d.join("subpasta")).unwrap();
        image::RgbImage::from_fn(4, 4, |x, _| image::Rgb([(x * 60) as u8, 20, 30]))
            .save_with_format(d.join("foto.png"), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(d.join("filme.mp4"), b"x").unwrap();
        std::fs::write(d.join("podre.png"), b"isto nao e um png").unwrap();
        d
    }

    /// A galeria desenha sem janela: monta o app no modo galeria, roda alguns
    /// frames em `Context::run_ui` e confere que a grade pediu (e recebeu) as
    /// miniaturas. Pega pânico de layout/ID e exercita breadcrumb + chips.
    #[test]
    fn galeria_desenha_a_grade_sem_panico() {
        let d = pasta_com_midia("grade");
        let cfg = Config::default();
        let mut app = ImageApp::new(cfg, None, false);
        // Boot sem arquivo = home é a galeria.
        assert!(app.mode == Mode::Gallery);
        app.dir = d.to_str().unwrap().to_string();
        let listing = img::list_entries(&app.dir).unwrap();
        app.dirs = listing.dirs;
        app.files = listing.files;
        assert_eq!(app.files.len(), 3, "foto + vídeo + foto podre");
        assert_eq!(app.dirs.len(), 1, "a subpasta vem separada");
        assert_eq!(app.dirs[0].file_name().unwrap(), "subpasta");

        let ctx = egui::Context::default();
        theme::apply(&ctx, cfg.theme);
        let input =
            || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };

        let mut pediu_thumb = false;
        for _ in 0..40 {
            let mut out = ctx.run_ui(input(), |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| app.gallery_ui(ui, &ctx, true));
            });
            // Sem renderer: os deltas de textura (que o wgpu aplicaria) são
            // descartados explicitamente — `TexturesDelta` dropado "sujo"
            // entra em pânico.
            out.textures_delta.clear();
            // O mesmo dreno do `ui()`: as threads de miniatura rodam de verdade.
            pediu_thumb |= !app.pending.is_empty();
            app.drain_thumbs(&ctx);
            if !app.thumbs.is_empty() && !app.thumb_failed.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(pediu_thumb, "a grade tem que pedir a miniatura dos tiles visíveis");
        assert_eq!(app.thumbs.len(), 1, "só a foto boa vira textura (vídeo não decodifica)");
        assert_eq!(app.thumb_failed.len(), 1, "a foto podre cai no fallback");

        // Miniatura que falhou NÃO é pedida de novo (senão vira uma thread
        // por frame, pra sempre): depois de um frame, nada pendente.
        let mut out = ctx.run_ui(input(), |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| app.gallery_ui(ui, &ctx, true));
        });
        out.textures_delta.clear();
        assert!(app.pending.is_empty(), "nem a boa nem a podre voltam pro pending");

        // Busca filtra pelo nome sem caixa e sem acento do SO.
        app.search = "FOTO".to_string();
        assert_eq!(app.visible_files().len(), 1);
        app.search = "nada-a-ver".to_string();
        assert!(app.visible_files().is_empty());
        // Uma busca sem resultado também não pode quebrar o desenho.
        let mut out = ctx.run_ui(input(), |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| app.gallery_ui(ui, &ctx, true));
        });
        out.textures_delta.clear();

        std::fs::remove_dir_all(&d).ok();
    }

    /// Setas da grade: o foco anda na lista filtrada e a seleção fica só nele.
    #[test]
    fn setas_movem_o_foco_na_grade() {
        let d = pasta_com_midia("setas");
        let mut app = ImageApp::new(Config::default(), None, false);
        app.dir = d.to_str().unwrap().to_string();
        app.files = img::list_entries(&app.dir).unwrap().files;
        app.grid_cols = 1; // uma coluna: ←/→ andam um item, como o teste espera
        let visible = app.visible_files();
        let ctx = egui::Context::default();

        // Um frame com a tecla apertada (o `key_pressed` vale dentro do pass).
        fn apertar(ctx: &egui::Context, app: &mut ImageApp, visible: &[usize], key: egui::Key) {
            let mut input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
            input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
            let mut out = ctx.run_ui(input, |ui| app.gallery_keys(ui.ctx(), visible));
            out.textures_delta.clear();
        }

        // ← sem foco cai no primeiro item.
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowLeft);
        assert_eq!(app.focus.as_ref(), Some(&app.files[0]));
        assert_eq!(app.selection.len(), 1);

        // → anda um e a seleção acompanha (só o focado).
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowRight);
        assert_eq!(app.focus.as_ref(), Some(&app.files[1]));
        assert_eq!(app.selection.len(), 1);
        assert!(app.selection.contains(&app.files[1]));

        // Clamp na borda direita: não dá a volta.
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowRight);
        assert_eq!(app.focus.as_ref(), Some(&app.files[2]));
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowRight);
        assert_eq!(app.focus.as_ref(), Some(&app.files[2]), "no último, → não anda");
        // Na borda esquerda, idem.
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowLeft);
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowLeft);
        apertar(&ctx, &mut app, &visible, egui::Key::ArrowLeft);
        assert_eq!(app.focus.as_ref(), Some(&app.files[0]), "no primeiro, ← não anda");

        std::fs::remove_dir_all(&d).ok();
    }

    /// Os 4 diálogos de arquivo desenham sem pânico (Window + moldura do kit,
    /// vários frames — ID duplicado e layout estourado apareceriam aqui).
    #[test]
    fn dialogos_de_arquivo_desenham_sem_panico() {
        let d = pasta_com_midia("dialogos");
        let cfg = Config::default();
        let mut app = ImageApp::new(cfg, None, false);
        app.dir = d.to_str().unwrap().to_string();
        let listing = img::list_entries(&app.dir).unwrap();
        app.dirs = listing.dirs;
        app.files = listing.files;
        app.selection.insert(app.files[0].clone());

        let ctx = egui::Context::default();
        theme::apply(&ctx, cfg.theme);
        let input =
            || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
        // Três frames por diálogo: o primeiro abre (pede foco), os outros
        // redesenham com o campo como está.
        let desenhar = |app: &mut ImageApp, dialog: Dialog| {
            app.dialog = dialog;
            for _ in 0..3 {
                let mut out = ctx.run_ui(input(), |ui| {
                    let ctx = ui.ctx().clone();
                    app.gallery_dialog(&ctx);
                });
                out.textures_delta.clear();
            }
        };

        desenhar(&mut app, Dialog::Rename { field: "foto.png".to_string() });
        desenhar(&mut app, Dialog::NewFolder { field: String::new() });
        desenhar(&mut app, Dialog::MoveTo);
        desenhar(&mut app, Dialog::ConfirmDelete);
        // Fechado: o desenho é no-op (e não pode reservar área nenhuma).
        desenhar(&mut app, Dialog::None);

        std::fs::remove_dir_all(&d).ok();
    }

    /// As ações da galeria mexem no disco de verdade e continuam desfazíveis
    /// (menos o excluir, que é a lixeira do sistema — fora de teste).
    #[test]
    fn acoes_da_galeria_mexem_no_disco() {
        let d = pasta_com_midia("acoes");
        let mut app = ImageApp::new(Config::default(), None, false);
        app.dir = d.to_str().unwrap().to_string();
        let listing = img::list_entries(&app.dir).unwrap();
        app.dirs = listing.dirs;
        app.files = listing.files;
        let foto = d.join("foto.png");

        // Renomear o único selecionado: o path da lista troca e o desfazer
        // nasce com a op inversa.
        app.selection.insert(foto.clone());
        app.open_rename(); // o campo vem com o nome ATUAL
        match &app.dialog {
            Dialog::Rename { field } => assert_eq!(field, "foto.png"),
            _ => panic!("o F2/clique em Renomear abre o diálogo de nome"),
        }
        app.apply_rename("renomeada.png");
        assert!(d.join("renomeada.png").exists() && !foto.exists());
        assert!(app.status.starts_with('✓'), "status: {}", app.status);
        assert!(app.selection.is_empty(), "a seleção morre junto com o path antigo");
        assert!(!app.undo.is_empty());
        // O `finish_ops` re-lista a pasta (o List também traz as subpastas).
        assert!(app.rx.is_some(), "o lote pede a re-listagem");

        app.undo_last();
        assert!(foto.exists() && !d.join("renomeada.png").exists(), "o desfazer volta o nome");
        assert!(app.undo.is_empty());

        // Mover pra subpasta (mesmo volume) e desfazer.
        app.selection.insert(foto.clone());
        app.apply_move(&d.join("subpasta"));
        assert!(d.join("subpasta").join("foto.png").exists());
        app.undo_last();
        assert!(foto.exists(), "o desfazer traz de volta pra pasta de origem");

        // Pasta nova com nome inválido não cria nada e sai no status.
        app.apply_new_folder("sub/ilegal");
        assert!(app.status.starts_with('⚠'), "status: {}", app.status);
        app.apply_new_folder("viagem");
        assert!(d.join("viagem").is_dir());

        std::fs::remove_dir_all(&d).ok();
    }
}
