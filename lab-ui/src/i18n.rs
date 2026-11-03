//! i18n via craft-i18n (padrão storytold): catálogos TSV em `i18n/locales/` embutidos
//! pelo `build.rs` (fonte: `scripts/gen_i18n.py` converte este enum). O enum `Key`
//! fica como API tipada (compilador cobre completude); o lookup é o motor do
//! craft-i18n, com fallback `pt-br → pt → en` e ausência de tradução = inglês.

use std::sync::LazyLock;

use craft_i18n::I18n;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// Códigos dos idiomas oferecidos (mesmos do enum `Lang`).
const OFFERED: &[&str] = &["en", "pt-br", "es"];

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, OFFERED);
    for p in problems {
        eprintln!("lab-ui i18n: {p}");
    }
    i18n
});

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    Pt,
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Pt, Lang::En, Lang::Es];

    /// Nome no seletor — sempre endônimo (regra da suíte).
    pub fn label(self) -> &'static str {
        match self {
            Lang::Pt => "Português",
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    // compartilhadas
    Language,
    Theme,
    Error,
    Search,
    Copy,
    // lab-monitor
    Cpu,
    Memory,
    Cores,
    Uptime,
    Disks,
    Processes,
    Filter,
    Name,
    End,
    KillTitle,
    KillAsk,
    Confirm,
    Cancel,
    Clear,
    // lab-clip
    ShowHide,
    Quit,
    Pin,
    Unpin,
    Delete,
    Empty,
    // lab-keys
    Vault,
    MasterPassword,
    Unlock,
    NewVault,
    WrongPassword,
    Add,
    Save,
    Items,
    Username,
    Password,
    Lock,
    // lab-keys onda 4
    Totp,
    QuickUnlock,
    ForgetKey,
    Edit,
    SecondsShort,
    // lab-keys: confirmação de exclusão
    TrashHint,
    // lab-keys onda 6 — importação, gerador, anexos, senha mestra e auto-type
    Import,
    ImportKdbx,
    ImportBitwarden,
    ImportCsv,
    ImportFile,
    ImportPassword,
    Imported,
    SkippedCardIdentity,
    ImportReportTitle,
    Generator,
    Generate,
    Use,
    Passphrase,
    Words,
    Separator,
    Capitalize,
    IncludeNumber,
    Length,
    Lowercase,
    Uppercase,
    Digits,
    Symbols,
    Strength0,
    Strength1,
    Strength2,
    Strength3,
    Strength4,
    ChangeMaster,
    OldPassword,
    NewPassword,
    NewPasswordConfirm,
    MasterChanged,
    Attachments,
    AddAttachment,
    SaveAttachment,
    NoAttachments,
    // lab-hub
    Install,
    Update,
    Open,
    StartMenu,
    Desktop,
    Installed,
    Available,
    Downloading,
    Refresh,
    UpToDate,
    NotInstalled,
    // lab-hub onda 6
    Uninstall,
    Clean,
    OpenFolder,
    UninstallAsk,
    // lab-hub: card do próprio hub
    Running,
    Restart,
    // lab-clip: preferências (atalho/autostart/bandeja)
    Settings,
    Hotkey,
    Define,
    Autostart,
    PressKeys,
    CloseToTray,
    // lab-keys onda 17 — spinner dos jobs pesados
    Working,
    Unlocking,
    Importing,
    ReadingFile,
    ChangingMaster,
    AttachmentTooBig,
    // lab-keys onda 23 — URL/notas no form, export, favorito, lixeira, reveal
    Url,
    Notes,
    Favorite,
    Reveal,
    Export,
    ExportJson,
    ExportCsv,
    ExportWarning,
    Exported,
    Trash,
    TrashEmpty,
    Restore,
    Restored,
    DeleteForever,
    Deleted,
    Close,
    // lab-keys: pastas + filtro por anexos
    Folders,
    Folder,
    NewFolder,
    RenameFolder,
    DeleteFolder,
    NoFolder,
    FolderName,
    FilterAttachments,
    DeleteAttachment,
    AllItems,
    // lab-keys: verificação de atualização (o prompt é o dialog do craft_update)
    CheckUpdates,
    // lab-player
    DragMediaHere,
    AddFile,
    AddFolder,
    Rotate,
    Mute,
    Unmute,
    MpvDownloading,
    MpvClosed,
    MpvNotInPath,
    // local-image
    ExportClean,
    VideoHint,
    // local-image: modo galeria
    Gallery,
    NoMedia,
    VideoTile,
    // local-image: gerenciamento de arquivos da galeria
    Rename,
    MoveTo,
    Undo,
    DeleteToTrashHint,
    // galeria QA (painel EXIF/estado vazio)
    NoExif,
    NoImage,
    PickFolder,
    // galeria QA: erros do fsops traduzidos na borda da UI (o fsops devolve
    // enum estruturado; o texto que o usuário lê nasce aqui)
    FsErrEmptyName,
    FsErrSeparator,
    FsErrDotDot,
    FsErrExists,
    FsErrCrossVolume,
    FsErrCancelled,
    FsErrNoGio,
    FsErrRename,
    FsErrMove,
    FsErrCreate,
    FsErrUndo,
    FsErrTrash,
    FsErrNoParent,
    FsErrNoName,
    // galeria QA: hovers da toolbar do viewer e da pill da galeria
    RotateHint,
    ExportCleanHint,
    CenterHint,
    GalleryHint,
}

/// Todas as chaves — usado pelo teste de completude (fora de teste o match
/// exaustivo do `t()` já cobre; o const fica silenciado pra não warnar na
/// build da lib).
#[cfg_attr(not(test), allow(dead_code))]
const KEYS: &[Key] = &[
    Key::Language,
    Key::Theme,
    Key::Error,
    Key::Search,
    Key::Copy,
    Key::Cpu,
    Key::Memory,
    Key::Cores,
    Key::Uptime,
    Key::Disks,
    Key::Processes,
    Key::Filter,
    Key::Name,
    Key::End,
    Key::KillTitle,
    Key::KillAsk,
    Key::Confirm,
    Key::Cancel,
    Key::Clear,
    Key::ShowHide,
    Key::Quit,
    Key::Pin,
    Key::Unpin,
    Key::Delete,
    Key::Empty,
    Key::Vault,
    Key::MasterPassword,
    Key::Unlock,
    Key::NewVault,
    Key::WrongPassword,
    Key::Add,
    Key::Save,
    Key::Items,
    Key::Username,
    Key::Password,
    Key::Lock,
    Key::Totp,
    Key::QuickUnlock,
    Key::ForgetKey,
    Key::Edit,
    Key::SecondsShort,
    Key::TrashHint,
    Key::Import,
    Key::ImportKdbx,
    Key::ImportBitwarden,
    Key::ImportCsv,
    Key::ImportFile,
    Key::ImportPassword,
    Key::Imported,
    Key::SkippedCardIdentity,
    Key::ImportReportTitle,
    Key::Generator,
    Key::Generate,
    Key::Use,
    Key::Passphrase,
    Key::Words,
    Key::Separator,
    Key::Capitalize,
    Key::IncludeNumber,
    Key::Length,
    Key::Lowercase,
    Key::Uppercase,
    Key::Digits,
    Key::Symbols,
    Key::Strength0,
    Key::Strength1,
    Key::Strength2,
    Key::Strength3,
    Key::Strength4,
    Key::ChangeMaster,
    Key::OldPassword,
    Key::NewPassword,
    Key::NewPasswordConfirm,
    Key::MasterChanged,
    Key::Attachments,
    Key::AddAttachment,
    Key::SaveAttachment,
    Key::NoAttachments,
    Key::Install,
    Key::Update,
    Key::Open,
    Key::StartMenu,
    Key::Desktop,
    Key::Installed,
    Key::Available,
    Key::Downloading,
    Key::Refresh,
    Key::UpToDate,
    Key::NotInstalled,
    Key::Uninstall,
    Key::Clean,
    Key::OpenFolder,
    Key::UninstallAsk,
    Key::Running,
    Key::Restart,
    Key::Settings,
    Key::Hotkey,
    Key::Define,
    Key::Autostart,
    Key::PressKeys,
    Key::CloseToTray,
    Key::Working,
    Key::Unlocking,
    Key::Importing,
    Key::ReadingFile,
    Key::ChangingMaster,
    Key::AttachmentTooBig,
    Key::Url,
    Key::Notes,
    Key::Favorite,
    Key::Reveal,
    Key::Export,
    Key::ExportJson,
    Key::ExportCsv,
    Key::ExportWarning,
    Key::Exported,
    Key::Trash,
    Key::TrashEmpty,
    Key::Restore,
    Key::Restored,
    Key::DeleteForever,
    Key::Deleted,
    Key::Close,
    Key::Folders,
    Key::Folder,
    Key::NewFolder,
    Key::RenameFolder,
    Key::DeleteFolder,
    Key::NoFolder,
    Key::FolderName,
    Key::FilterAttachments,
    Key::DeleteAttachment,
    Key::AllItems,
    Key::CheckUpdates,
    Key::DragMediaHere,
    Key::AddFile,
    Key::AddFolder,
    Key::Rotate,
    Key::Mute,
    Key::Unmute,
    Key::MpvDownloading,
    Key::MpvClosed,
    Key::MpvNotInPath,
    Key::ExportClean,
    Key::VideoHint,
    Key::Gallery,
    Key::NoMedia,
    Key::VideoTile,
    Key::Rename,
    Key::MoveTo,
    Key::Undo,
    Key::DeleteToTrashHint,
    Key::NoExif,
    Key::NoImage,
    Key::PickFolder,
    Key::FsErrEmptyName,
    Key::FsErrSeparator,
    Key::FsErrDotDot,
    Key::FsErrExists,
    Key::FsErrCrossVolume,
    Key::FsErrCancelled,
    Key::FsErrNoGio,
    Key::FsErrRename,
    Key::FsErrMove,
    Key::FsErrCreate,
    Key::FsErrUndo,
    Key::FsErrTrash,
    Key::FsErrNoParent,
    Key::FsErrNoName,
    Key::RotateHint,
    Key::ExportCleanHint,
    Key::CenterHint,
    Key::GalleryHint,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    let code = match lang {
        Lang::Pt => "pt-br",
        Lang::En => "en",
        Lang::Es => "es",
    };
    // A fonte inglesa é a chave de lookup do catálogo (padrão craft-i18n).
    let source = match key {
        Key::Language => "Language",
        Key::Theme => "Theme",
        Key::Error => "Error",
        Key::Search => "Search…",
        Key::Copy => "Copy",
        Key::Memory => "Memory",
        Key::Cores => "Cores",
        Key::Uptime => "Uptime",
        Key::Disks => "Disks",
        Key::Processes => "Processes",
        Key::Filter => "Filter",
        Key::Name => "Name",
        Key::End => "End task",
        Key::KillTitle => "End process?",
        Key::KillAsk => "Unsaved work in that app is lost.",
        Key::Confirm => "Confirm",
        Key::Cancel => "Cancel",
        Key::Clear => "Clear",
        Key::ShowHide => "Show/Hide",
        Key::Quit => "Quit",
        Key::Pin => "Pin",
        Key::Unpin => "Unpin",
        Key::Delete => "Delete",
        Key::Empty => "empty — copy something",
        Key::Vault => "Vault",
        Key::MasterPassword => "Master password",
        Key::Unlock => "Unlock",
        Key::NewVault => "New vault",
        Key::WrongPassword => "wrong password or corrupted/tampered file",
        Key::Add => "Add",
        Key::Save => "Save",
        Key::Items => "Items",
        Key::Username => "Username",
        Key::Password => "Password",
        Key::Lock => "Lock",
        Key::QuickUnlock => "Quick unlock on this PC",
        Key::ForgetKey => "Forget key",
        Key::Edit => "Edit",
        Key::Install => "Install",
        Key::Update => "Update",
        Key::Open => "Open",
        Key::StartMenu => "Start menu",
        Key::Desktop => "Desktop",
        Key::Installed => "installed",
        Key::Available => "available",
        Key::Downloading => "downloading",
        Key::Refresh => "Refresh",
        Key::UpToDate => "up to date",
        Key::NotInstalled => "not installed",
        Key::Uninstall => "Uninstall",
        Key::Clean => "Cleanup",
        Key::OpenFolder => "Open folder",
        Key::UninstallAsk => "Uninstall and remove shortcuts?",
        Key::Running => "running",
        Key::Restart => "restart Lab Hub",
        Key::TrashHint => "The item goes to trash — recoverable in the official LocalKeys.",
        Key::Import => "Import",
        Key::ImportBitwarden => "Bitwarden (encrypted export)",
        Key::ImportFile => "File",
        Key::ImportPassword => "File password",
        Key::Imported => "items imported",
        Key::SkippedCardIdentity => "card/identity skipped — the lab only manages login/note",
        Key::ImportReportTitle => "Import report",
        Key::Generator => "Generator",
        Key::Generate => "Generate",
        Key::Use => "Use",
        Key::Passphrase => "Passphrase",
        Key::Words => "Words",
        Key::Separator => "Separator",
        Key::Capitalize => "Capitalize",
        Key::IncludeNumber => "Include number",
        Key::Length => "Length",
        Key::Lowercase => "Lowercase",
        Key::Uppercase => "Uppercase",
        Key::Digits => "Digits",
        Key::Symbols => "Symbols",
        Key::Strength0 => "terrible",
        Key::Strength1 => "weak",
        Key::Strength2 => "fair",
        Key::Strength3 => "strong",
        Key::Strength4 => "excellent",
        Key::ChangeMaster => "Change master password",
        Key::OldPassword => "Current password",
        Key::NewPassword => "New password",
        Key::NewPasswordConfirm => "Confirm new password",
        Key::MasterChanged => "master password changed",
        Key::Attachments => "Attachments",
        Key::AddAttachment => "Add attachment",
        Key::SaveAttachment => "Save attachment",
        Key::NoAttachments => "no attachments",
        Key::Settings => "Settings",
        Key::Hotkey => "Global hotkey",
        Key::Define => "Set…",
        Key::Autostart => "Start with system",
        Key::PressKeys => "press the keys…",
        Key::CloseToTray => "Closing minimizes to tray.",
        Key::Working => "working…",
        Key::Unlocking => "unlocking…",
        Key::Importing => "importing…",
        Key::ReadingFile => "reading file…",
        Key::ChangingMaster => "changing the master password…",
        Key::AttachmentTooBig => "attachment too large (max 10 MiB — the whole vault is rewritten on every save)",
        Key::Url => "URL",
        Key::Notes => "Notes",
        Key::Favorite => "Favorite",
        Key::Reveal => "Reveal",
        Key::Export => "Export",
        Key::ExportJson => "JSON (vault in the clear)",
        Key::ExportCsv => "CSV (spreadsheet)",
        Key::ExportWarning => "WARNING: the export is UNENCRYPTED — any app can read it. Use it only to migrate, then delete the file.",
        Key::Exported => "exported",
        Key::Trash => "Trash",
        Key::TrashEmpty => "trash is empty",
        Key::Restore => "Restore",
        Key::Restored => "restored",
        Key::DeleteForever => "Delete forever",
        Key::Deleted => "deleted forever",
        Key::Close => "Close",
        Key::Folders => "Folders",
        Key::Folder => "Folder",
        Key::NewFolder => "New folder",
        Key::RenameFolder => "Rename folder",
        Key::DeleteFolder => "Delete folder",
        Key::NoFolder => "No folder",
        Key::FolderName => "Folder name",
        Key::FilterAttachments => "Filter: attachments",
        Key::DeleteAttachment => "Delete attachment",
        Key::AllItems => "All",
        Key::DragMediaHere => "Drag media here or pick from the playlist",
        Key::AddFile => "Add file",
        Key::AddFolder => "Add folder",
        Key::Rotate => "Rotate",
        Key::Mute => "Mute (M)",
        Key::Unmute => "Unmute (M)",
        Key::MpvDownloading => "Downloading mpv…",
        Key::MpvClosed => "mpv closed",
        Key::MpvNotInPath => "mpv is not on PATH — install it (e.g. sudo apt install mpv)",
        Key::Cpu => "CPU",
        Key::Totp => "TOTP",
        Key::SecondsShort => "s",
        Key::ImportKdbx => "KeePass (.kdbx)",
        Key::ImportCsv => "CSV (Chrome/Edge/LastPass/1Password)",
        Key::CheckUpdates => "Check for updates",
        Key::ExportClean => "Export clean",
        Key::VideoHint => "video — opened in the system player",
        Key::Gallery => "Gallery",
        Key::NoMedia => "no photos or videos here",
        Key::VideoTile => "video",
        Key::Rename => "Rename",
        Key::MoveTo => "Move to…",
        Key::Undo => "Undo",
        Key::DeleteToTrashHint => "Goes to the recycle bin — recoverable there.",
        Key::NoExif => "no EXIF",
        Key::NoImage => "no image",
        Key::PickFolder => "open a folder to browse",
        Key::FsErrEmptyName => "empty name",
        // `\\)` no literal = `\)` na fonte: é a contrabarra que o usuário vê.
        Key::FsErrSeparator => "the name cannot contain a path separator (/ or \\)",
        Key::FsErrDotDot => "the name cannot contain \"..\"",
        // Fonte termina com ": " porque o nome do arquivo entra em seguida.
        Key::FsErrExists => "an item with this name already exists: ",
        Key::FsErrCrossVolume => "moving between drives is not supported yet — copy the file manually",
        Key::FsErrCancelled => "deletion cancelled",
        Key::FsErrNoGio => "gio (trash) not found — install gvfs",
        Key::FsErrRename => "rename",
        Key::FsErrMove => "move",
        Key::FsErrCreate => "create folder",
        Key::FsErrUndo => "undo",
        Key::FsErrTrash => "recycle bin",
        Key::FsErrNoParent => "file has no parent folder",
        Key::FsErrNoName => "file has no name",
        Key::RotateHint => "rotate 90° ([ and ] also rotate)",
        Key::ExportCleanHint => "re-encodes without EXIF — saves nome.lab.png next to the original",
        Key::CenterHint => "center (C)",
        Key::GalleryHint => "browse folders (Esc returns to the photo)",
    };
    let craft_lang = I18N.from_code(code).unwrap_or_else(|| I18N.from_code("en").expect("en é builtin"));
    I18N.t(craft_lang, source)
}

/// Contagem de itens com a pluralização do motor (`trn`): "1 item" / "7 items".
/// O `format!` de antes colava o número no rótulo fixo ("1 Items"); as formas
/// inglesas são também a chave da linha `@plural` do catálogo.
pub fn t_items(lang: Lang, n: usize) -> String {
    let code = match lang {
        Lang::Pt => "pt-br",
        Lang::En => "en",
        Lang::Es => "es",
    };
    let craft_lang = I18N.from_code(code).unwrap_or_else(|| I18N.from_code("en").expect("en é builtin"));
    I18N.trn(craft_lang, n as u64, "{n} item", "{n} items")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equivalente do "tsc força completude": toda chave tem tradução nos
    /// 3 idiomas (match exaustivo + não-vazio).
    #[test]
    fn todas_as_chaves_tem_traducao() {
        for lang in Lang::ALL {
            for key in KEYS {
                let s = t(lang, *key);
                assert!(!s.is_empty(), "chave {key:?} vazia em {lang:?}");
            }
        }
    }

    /// O motor craft-i18n está ligado: pt-br sai do catálogo TSV, não do inglês.
    #[test]
    fn craft_i18n_busca_no_catalogo() {
        assert_eq!(t(Lang::Pt, Key::Vault), "Cofre");
        assert_eq!(t(Lang::Es, Key::Vault), "Bóveda");
        assert_eq!(t(Lang::En, Key::Vault), "Vault");
    }

    /// As chaves da galeria saem do TSV nos 3 idiomas (não sobrou em inglês).
    #[test]
    fn galeria_traduzida() {
        assert_eq!(t(Lang::En, Key::Gallery), "Gallery");
        assert_eq!(t(Lang::Pt, Key::Gallery), "Galeria");
        assert_eq!(t(Lang::Es, Key::Gallery), "Galería");
        assert_eq!(t(Lang::Pt, Key::NoMedia), "sem fotos ou vídeos aqui");
        assert_eq!(t(Lang::Es, Key::NoMedia), "sin fotos ni vídeos aquí");
        assert_eq!(t(Lang::Pt, Key::VideoTile), "vídeo");
        assert_eq!(t(Lang::Es, Key::VideoTile), "vídeo");
        // Gerenciamento de arquivos: as 4 chaves novas saem do TSV nos 3
        // idiomas (não sobrou em inglês).
        assert_eq!(t(Lang::Pt, Key::Rename), "Renomear");
        assert_eq!(t(Lang::Es, Key::Rename), "Renombrar");
        assert_eq!(t(Lang::Pt, Key::MoveTo), "Mover para…");
        assert_eq!(t(Lang::Es, Key::MoveTo), "Mover a…");
        assert_eq!(t(Lang::Pt, Key::Undo), "Desfazer");
        assert_eq!(t(Lang::Es, Key::Undo), "Deshacer");
        assert_eq!(
            t(Lang::Pt, Key::DeleteToTrashHint),
            "Vai para a lixeira — dá para recuperar lá."
        );
        assert_eq!(
            t(Lang::Es, Key::DeleteToTrashHint),
            "Va a la papelera de reciclaje — puedes recuperarlo allí."
        );
        assert_eq!(t(Lang::En, Key::DeleteToTrashHint), "Goes to the recycle bin — recoverable there.");
    }

    /// Contagem com plural do motor: singular e plural saem das formas do
    /// catálogo (`@plural {n} item|{n} items`), não de um rótulo fixo.
    #[test]
    fn contagem_de_itens_tem_plural() {
        let um = t_items(Lang::Pt, 1);
        assert!(um.contains("item") && !um.contains("itens"), "singular pt: {um}");
        let sete = t_items(Lang::Pt, 7);
        assert!(sete.contains("itens"), "plural pt: {sete}");
        // InglêS: as formas da fonte, com o número preenchido.
        assert_eq!(t_items(Lang::En, 1), "1 item");
        assert_eq!(t_items(Lang::En, 7), "7 items");
        // Espanhol pega a linha `@plural` do catálogo es.
        assert!(t_items(Lang::Es, 7).contains("elementos"), "plural es: {}", t_items(Lang::Es, 7));
    }

    /// As chaves novas (erros do fsops + hovers) saem do TSV nos 2 catálogos —
    /// se uma linha caísse na validação do craft-i18n, o teste veria o inglês.
    #[test]
    fn erros_e_hovers_traduzidos() {
        assert_eq!(t(Lang::Pt, Key::FsErrEmptyName), "nome vazio");
        assert_eq!(t(Lang::Es, Key::FsErrEmptyName), "nombre vacío");
        // A contrabarra é `\\)` no TSV (escape do catálogo) e `\` na tela.
        assert_eq!(t(Lang::Pt, Key::FsErrSeparator), "o nome não pode ter separador de pasta (/ ou \\)");
        assert_eq!(t(Lang::Pt, Key::FsErrDotDot), "o nome não pode ter \"..\"");
        assert_eq!(t(Lang::Pt, Key::FsErrExists), "já existe um item chamado ");
        assert_eq!(t(Lang::Es, Key::FsErrExists), "ya existe un elemento llamado ");
        assert_eq!(t(Lang::Pt, Key::FsErrCrossVolume), "mover entre discos ainda não existe neste app — copie por fora se precisar");
        assert_eq!(t(Lang::Pt, Key::FsErrCancelled), "exclusão cancelada");
        assert_eq!(t(Lang::Pt, Key::FsErrNoGio), "gio (trash) não encontrado — instale gvfs");
        assert_eq!(t(Lang::Pt, Key::FsErrRename), "renomear");
        assert_eq!(t(Lang::Es, Key::FsErrMove), "mover");
        assert_eq!(t(Lang::Pt, Key::FsErrCreate), "criar pasta");
        assert_eq!(t(Lang::Pt, Key::FsErrUndo), "desfazer");
        assert_eq!(t(Lang::Pt, Key::FsErrTrash), "lixeira");
        assert_eq!(t(Lang::Pt, Key::FsErrNoParent), "arquivo sem pasta-pai");
        assert_eq!(t(Lang::Es, Key::FsErrNoName), "el archivo no tiene nombre");
        assert_eq!(t(Lang::Pt, Key::RotateHint), "girar 90° ([ e ] também giram)");
        assert_eq!(t(Lang::Es, Key::RotateHint), "girar 90° ([ y ] también giran)");
        assert_eq!(t(Lang::Pt, Key::ExportCleanHint), "re-encoda sem EXIF — salva nome.lab.png ao lado");
        assert_eq!(t(Lang::Pt, Key::CenterHint), "centralizar (C)");
        assert_eq!(t(Lang::Es, Key::GalleryHint), "navegación por carpetas (Esc vuelve a la foto)");
        // Fonte única (item 7): as duplicatas herdadas do lab-keys não voltam.
        assert_eq!(t(Lang::En, Key::ImportReportTitle), "Import report");
        assert_eq!(t(Lang::En, Key::SaveAttachment), "Save attachment");
        assert_eq!(t(Lang::En, Key::FilterAttachments), "Filter: attachments");
        assert_eq!(t(Lang::En, Key::Reveal), "Reveal");
        assert_eq!(t(Lang::Pt, Key::ImportReportTitle), "Importação");
        assert_eq!(t(Lang::Pt, Key::FilterAttachments), "Anexos");
    }
}
