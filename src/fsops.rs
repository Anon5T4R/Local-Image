//! Operações de arquivo da galeria: renomear, mover (mesmo volume), excluir
//! pra LIXEIRA do sistema e criar pasta — mais o log de desfazer da sessão.
//!
//! Tudo aqui é operação de METADADO (`rename`/`create_dir`) ou uma chamada à
//! lixeira do sistema. São instantâneas (nada de copiar bytes), então rodam
//! direto na thread da UI, sem job e sem canal.
//!
//! O desfazer é da SESSÃO e cobre só o que dá pra reverter com um rename:
//! mover e renomear. O delete NÃO entra nele — a lixeira do sistema já é o
//! desfazer dele, e é lá que o usuário recupera.

use std::path::{Path, PathBuf};

/// Erro das operações de arquivo, ESTRUTURADO: o `fn` que chama decide o
/// texto final. O `Display` mantém os textos em PORTUGUÊS de antes (é o que os
/// testes e o log conhecem); a UI traduz variante por variante na borda
/// (`fs_err` no `main.rs`), que é onde o idioma do usuário vive.
#[derive(Debug)]
pub enum FsError {
    EmptyName,
    Separator,
    DotDot,
    NoParent,
    NoName,
    /// O nome que colidiu — o chamador só emenda o resto da frase.
    Exists(String),
    /// `rename` não cruza volume: o caso que merece explicação própria.
    CrossVolume,
    RenameIo(std::io::Error),
    MoveIo(std::io::Error),
    CreateIo(std::io::Error),
    UndoIo(std::io::Error),
    /// Código que o `SHFileOperationW` devolveu (só Windows).
    #[cfg(windows)]
    TrashCode(i32),
    /// O SO abortou a exclusão (só Windows: `fAnyOperationsAborted`).
    #[cfg(windows)]
    Aborted,
    /// stderr do `gio` (só Linux).
    #[cfg(not(windows))]
    TrashOut(String),
    #[cfg(not(windows))]
    NoGio,
    #[cfg(not(windows))]
    TrashIo(std::io::Error),
}

impl FsError {
    /// Classifica o erro do `rename`: discos diferentes é o caso especial (o
    /// `rename` só vale dentro do MESMO volume, e cruzar exigiria
    /// copiar+apagar — fora desta fatia); o resto é o io cru.
    fn from_move(e: std::io::Error) -> FsError {
        match e.raw_os_error() {
            // ERROR_NOT_SAME_DEVICE (Windows) / EXDEV (Linux).
            Some(17) | Some(18) => FsError::CrossVolume,
            _ => FsError::MoveIo(e),
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::EmptyName => f.write_str("nome vazio"),
            FsError::Separator => f.write_str("o nome não pode ter separador de pasta (/ ou \\)"),
            FsError::DotDot => f.write_str("o nome não pode ter \"..\""),
            FsError::NoParent => f.write_str("arquivo sem pasta-pai"),
            FsError::NoName => f.write_str("arquivo sem nome"),
            FsError::Exists(name) => write!(f, "já existe um item chamado {name}"),
            FsError::CrossVolume => f.write_str("mover entre discos ainda não existe neste app — copie por fora se precisar"),
            FsError::RenameIo(e) => write!(f, "renomear: {e}"),
            FsError::MoveIo(e) => write!(f, "mover: {e}"),
            FsError::CreateIo(e) => write!(f, "criar pasta: {e}"),
            FsError::UndoIo(e) => write!(f, "desfazer: {e}"),
            #[cfg(windows)]
            FsError::TrashCode(code) => write!(f, "lixeira: código {code}"),
            #[cfg(windows)]
            FsError::Aborted => f.write_str("exclusão cancelada"),
            #[cfg(not(windows))]
            FsError::TrashOut(msg) => write!(f, "lixeira: {msg}"),
            #[cfg(not(windows))]
            FsError::NoGio => f.write_str("gio (trash) não encontrado — instale gvfs"),
            #[cfg(not(windows))]
            FsError::TrashIo(e) => write!(f, "lixeira: {e}"),
        }
    }
}

/// Nome de arquivo/pasta aceito nas operações. Recusa separador (o "nome"
/// escaparia da pasta corrente) e `..` (idem, subindo um nível) — a validação
/// fica aqui, num lugar só, e não na UI que chama.
fn check_name(name: &str) -> Result<&str, FsError> {
    // Espaço sobrando nas pontas é digitação, não nome: o Windows nem cria
    // arquivo terminando em espaço.
    let name = name.trim();
    if name.is_empty() {
        return Err(FsError::EmptyName);
    }
    if name.contains('/') || name.contains('\\') {
        return Err(FsError::Separator);
    }
    if name.contains("..") || name == "." {
        return Err(FsError::DotDot);
    }
    Ok(name)
}

/// Renomeia `src` pra `new_name` NA MESMA pasta. Nome igual ao atual é no-op
/// (devolve o próprio path): o usuário só abriu o diálogo e confirmou.
pub fn rename_file(src: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    let name = check_name(new_name)?;
    let current = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name == current {
        return Ok(src.to_path_buf());
    }
    let parent = src.parent().ok_or(FsError::NoParent)?;
    let dst = parent.join(name);
    // `fs::rename` SOBRESCREVERIA o destino silenciosamente em alguns pares
    // de SO; recusar antes é o único jeito de não perder arquivo do usuário.
    if dst.exists() {
        // Windows é case-insensitive: `foto.png`→`Foto.png` enxerga o PRÓPRIO
        // arquivo no `dst.exists()`. Diferença SÓ de caixa passa por um nome
        // temporário único, em dois passos — e o 2º falhando reverte o 1º pro
        // arquivo não ficar com nome de trabalho.
        if !current.eq_ignore_ascii_case(name) {
            return Err(FsError::Exists(name.to_string()));
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let tmp = parent.join(format!(".{stamp}s-renomeando"));
        std::fs::rename(src, &tmp).map_err(FsError::RenameIo)?;
        if let Err(e) = std::fs::rename(&tmp, &dst) {
            let _ = std::fs::rename(&tmp, src);
            return Err(FsError::RenameIo(e));
        }
        return Ok(dst);
    }
    std::fs::rename(src, &dst).map_err(FsError::RenameIo)?;
    Ok(dst)
}

/// Move `src` pra dentro de `dst_dir` (mesmo volume — ver [`FsError::from_move`]).
/// Mesma pasta de origem = no-op, como no rename.
pub fn move_file(src: &Path, dst_dir: &Path) -> Result<PathBuf, FsError> {
    let file_name = src.file_name().ok_or(FsError::NoName)?;
    let dst = dst_dir.join(file_name);
    if dst == src {
        return Ok(src.to_path_buf());
    }
    if dst.exists() {
        return Err(FsError::Exists(file_name.to_string_lossy().into_owned()));
    }
    std::fs::rename(src, &dst).map_err(FsError::from_move)?;
    Ok(dst)
}

/// Manda `path` pra LIXEIRA do sistema (recuperável lá). Nunca apaga de vez:
/// exclusão definitiva é do usuário, no Explorer/gerenciador de arquivos.
pub fn delete_to_trash(path: &Path) -> Result<(), FsError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{
            SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
        };
        // `pFrom` é uma LISTA de paths double-null-terminated: o primeiro 0
        // fecha o path, o segundo fecha a lista. Sem o segundo a API segue
        // lendo memória além do buffer.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.push(0);
        from.push(0);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            // SILENT + NOERRORUI + NOCONFIRMATION: quem confirmou foi o
            // diálogo do app; a caixa do Explorer por cima seria ruído.
            // WANTNUKEWARNING: arquivo que NÃO cabe na lixeira (cota estourada,
            // caminho de rede) seria apagado DEFINITIVO em silêncio — a flag
            // faz o SO confirmar só esse caso (sobrepõe o NOCONFIRMATION).
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING).0 as u16,
            ..Default::default()
        };
        // FFI: a API só LÊ a struct e a string (que vivem até o fim do
        // bloco) e devolve código de erro — não guarda ponteiro nenhum.
        let code = unsafe { SHFileOperationW(&mut op) };
        if code != 0 {
            return Err(FsError::TrashCode(code));
        }
        // Cópia do campo antes do `as_bool` (na build x86 a struct é packed).
        let aborted = op.fAnyOperationsAborted;
        if aborted.as_bool() {
            return Err(FsError::Aborted);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // Sem FFI no Linux: o `gio` (gvfs) é quem fala com a lixeira do
        // desktop. `output` espera o processo terminar — o resultado importa.
        let out = std::process::Command::new("gio").arg("trash").arg(path).output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(FsError::TrashOut(String::from_utf8_lossy(&o.stderr).trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(FsError::NoGio),
            Err(e) => Err(FsError::TrashIo(e)),
        }
    }
}

/// Cria a pasta `name` dentro de `parent`. Mesmas regras de nome do rename
/// (e o destino já existente é erro, não "tudo bem, já está lá": o usuário
/// acabou de digitar um nome e merece saber que não criou nada).
pub fn create_dir(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let name = check_name(name)?;
    let dst = parent.join(name);
    if dst.exists() {
        return Err(FsError::Exists(name.to_string()));
    }
    std::fs::create_dir(&dst).map_err(FsError::CreateIo)?;
    Ok(dst)
}

/// Uma operação REVERSÍVEL da sessão (o delete fica de fora: a lixeira é o
/// desfazer dele). Guarda os dois lados porque o desfazer é renomear de
/// volta — nada de "recalcular" o destino depois.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Move { from: PathBuf, to: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
}

impl Op {
    /// A operação que desfaz esta (`to` → `from`). Pura: quem executa é o
    /// chamador, que assim sabe se deve devolver a op pro log.
    pub fn reverse(&self) -> Op {
        match self {
            Op::Move { from, to } => Op::Move { from: to.clone(), to: from.clone() },
            Op::Rename { from, to } => Op::Rename { from: to.clone(), to: from.clone() },
        }
    }

    fn endpoints(&self) -> (&Path, &Path) {
        match self {
            Op::Move { from, to } | Op::Rename { from, to } => (from, to),
        }
    }
}

/// Pilha de desfazer da sessão (o app é quem decide o que entra nela).
#[derive(Default)]
pub struct UndoLog {
    ops: Vec<Op>,
}

impl UndoLog {
    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// O que o próximo desfazer faria — o hover do botão mostra isso.
    pub fn peek(&self) -> Option<&Op> {
        self.ops.last()
    }

    /// Desfaz o topo (um rename de volta). Devolve a op original, pro
    /// chamador saber o que mudou. Falhou? A op volta pro log: nada foi
    /// desfeito e o usuário ainda pode tentar.
    pub fn undo(&mut self) -> Result<Option<Op>, FsError> {
        let Some(op) = self.ops.pop() else {
            return Ok(None);
        };
        let rev = op.reverse();
        let (from, to) = rev.endpoints();
        // Mesmo guard do rename/move: `fs::rename` SOBRESCREVE o destino no
        // Windows — desfazer não pode apagar em silêncio um arquivo novo que
        // surgiu no lugar do antigo (outro app recriou o nome, por exemplo).
        if to.exists() {
            self.ops.push(op);
            return Err(FsError::Exists(to.display().to_string()));
        }
        if let Err(e) = std::fs::rename(from, to) {
            self.ops.push(op);
            return Err(FsError::UndoIo(e));
        }
        Ok(Some(op))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pasta única por teste — temp do SO, mesmo volume (mover nunca cai no
    /// caso cross-device, que é justamente o que não dá pra testar aqui).
    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("local-image-fs-{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn nome_vazio_e_recusado() {
        let d = tmpdir("vazio");
        std::fs::write(d.join("a.png"), b"x").unwrap();
        assert!(rename_file(&d.join("a.png"), "").is_err());
        assert!(rename_file(&d.join("a.png"), "   ").is_err(), "só espaço também é vazio");
        assert!(create_dir(&d, "").is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn nome_com_separador_e_recusado() {
        let d = tmpdir("separador");
        std::fs::write(d.join("a.png"), b"x").unwrap();
        // Barra e contrabarra: no Windows as duas, no Linux a barra levaria
        // pra subpasta — nenhuma das duas pode passar.
        assert!(rename_file(&d.join("a.png"), "sub/a.png").is_err());
        assert!(rename_file(&d.join("a.png"), "sub\\a.png").is_err());
        assert!(create_dir(&d, "sub/a").is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn nome_com_dois_pontos_e_recusado() {
        let d = tmpdir("pontos");
        std::fs::write(d.join("a.png"), b"x").unwrap();
        assert!(rename_file(&d.join("a.png"), "..").is_err());
        assert!(rename_file(&d.join("a.png"), "../b.png").is_err());
        assert!(create_dir(&d, "..").is_err());
        // "." sozinho também não é nome de nada.
        assert!(create_dir(&d, ".").is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn rename_de_verdade_muda_o_nome() {
        let d = tmpdir("rename");
        let src = d.join("antes.png");
        std::fs::write(&src, b"x").unwrap();
        let dst = rename_file(&src, "depois.png").unwrap();
        assert_eq!(dst, d.join("depois.png"));
        assert!(!src.exists(), "o antigo sumiu");
        assert!(dst.exists(), "o novo está lá");
        // Mesmo nome = no-op com Ok do MESMO path.
        assert_eq!(rename_file(&dst, "depois.png").unwrap(), dst);
        // Destino ocupado é erro (não sobrescreve o do usuário).
        std::fs::write(d.join("outro.png"), b"y").unwrap();
        assert!(rename_file(&dst, "outro.png").is_err());
        assert_eq!(std::fs::read(d.join("outro.png")).unwrap(), b"y", "o vizinho continua intacto");
        std::fs::remove_dir_all(&d).ok();
    }

    /// Rename que muda SÓ a caixa (`foto.png`→`Foto.png`): no Windows o
    /// `dst.exists()` enxerga o PRÓPRIO arquivo (FS case-insensitive) e o
    /// caminho de um passo recusaria. O dois-passos por nome temporário
    /// resolve — e não pode deixar temporário para trás nem perder conteúdo.
    #[test]
    fn rename_so_caixa_muda_a_caixa() {
        let d = tmpdir("caixa");
        let src = d.join("foto.png");
        std::fs::write(&src, b"conteudo").unwrap();
        let dst = rename_file(&src, "Foto.png").unwrap();
        assert!(dst.exists(), "o destino existe");
        // O NTFS pode PRESERVAR a caixa antiga; o que o teste cobra é um
        // arquivo só, com o nome certo semanticamente e o conteúdo intacto.
        let nomes: Vec<String> =
            std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(nomes.len(), 1, "nenhum temporário sobrou: {nomes:?}");
        assert!(nomes[0].eq_ignore_ascii_case("foto.png"), "arquivo presente: {nomes:?}");
        assert_eq!(std::fs::read(d.join(&nomes[0])).unwrap(), b"conteudo", "conteúdo intacto");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn move_de_verdade_muda_de_pasta() {
        let d = tmpdir("move");
        let sub = d.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let src = d.join("foto.png");
        std::fs::write(&src, b"x").unwrap();
        let dst = move_file(&src, &sub).unwrap();
        assert_eq!(dst, sub.join("foto.png"));
        assert!(dst.exists() && !src.exists());
        // Mover pra pasta onde já está = no-op.
        assert_eq!(move_file(&dst, &sub).unwrap(), dst);
        // Colisão de nome na pasta de destino é erro claro.
        std::fs::write(d.join("foto.png"), b"z").unwrap();
        let err = move_file(&d.join("foto.png"), &sub).unwrap_err();
        assert!(err.to_string().contains("já existe"), "erro: {err}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn erro_de_disco_diferente_vira_mensagem_amigavel() {
        // 17 = ERROR_NOT_SAME_DEVICE no Windows (o caminho real do usuário).
        let msg = FsError::from_move(std::io::Error::from_raw_os_error(17)).to_string();
        assert!(msg.contains("entre discos"), "mensagem: {msg}");
        // Qualquer outro código cai no genérico, com o motivo do SO.
        let outro = FsError::from_move(std::io::Error::from_raw_os_error(5)).to_string();
        assert!(outro.starts_with("mover:"), "mensagem: {outro}");
        assert!(!outro.contains("entre discos"));
    }

    #[test]
    fn create_dir_de_verdade() {
        let d = tmpdir("nova");
        let nova = create_dir(&d, "viagem").unwrap();
        assert_eq!(nova, d.join("viagem"));
        assert!(nova.is_dir());
        // Já existe: erro, não silêncio.
        assert!(create_dir(&d, "viagem").is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reverse_inverte_os_dois_lados() {
        let op = Op::Rename { from: PathBuf::from("a"), to: PathBuf::from("b") };
        assert_eq!(op.reverse(), Op::Rename { from: PathBuf::from("b"), to: PathBuf::from("a") });
        let mv = Op::Move { from: PathBuf::from("x/f.png"), to: PathBuf::from("y/f.png") };
        assert_eq!(mv.reverse(), Op::Move { from: PathBuf::from("y/f.png"), to: PathBuf::from("x/f.png") });
        // Reverter duas vezes volta ao original (é o que o undo depende).
        assert_eq!(op.reverse().reverse(), op);
    }

    #[test]
    fn undo_log_empilha_e_desfaz() {
        let d = tmpdir("undo");
        let sub = d.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let a = d.join("a.png");
        std::fs::write(&a, b"x").unwrap();

        let mut log = UndoLog::default();
        assert!(log.is_empty());
        assert!(log.undo().unwrap().is_none(), "log vazio: nada a desfazer");

        // Rename e depois move: o desfazer volta na ordem inversa.
        let b = rename_file(&a, "b.png").unwrap();
        log.push(Op::Rename { from: a.clone(), to: b.clone() });
        let moved = move_file(&b, &sub).unwrap();
        log.push(Op::Move { from: b.clone(), to: moved.clone() });
        assert_eq!(log.peek(), Some(&Op::Move { from: b.clone(), to: moved.clone() }));

        assert_eq!(log.undo().unwrap(), Some(Op::Move { from: b.clone(), to: moved.clone() }));
        assert!(b.exists() && !moved.exists(), "o move foi desfeito");
        assert_eq!(log.undo().unwrap(), Some(Op::Rename { from: a.clone(), to: b.clone() }));
        assert!(a.exists() && !b.exists(), "o rename foi desfeito");
        assert!(log.is_empty());
        assert!(log.undo().unwrap().is_none());

        // Desfazer que FALHA (origem sumiu) devolve a op pro log.
        log.push(Op::Rename { from: d.join("sumiu.png"), to: d.join("nada.png") });
        assert!(log.undo().is_err());
        assert!(!log.is_empty(), "a op continua no log: nada foi desfeito");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn undo_nao_sobrescreve_destino_ocupado() {
        let d = tmpdir("undo-guard");
        let a = d.join("a.png");
        std::fs::write(&a, b"x").unwrap();
        let b = rename_file(&a, "b.png").unwrap();
        let mut log = UndoLog::default();
        log.push(Op::Rename { from: a.clone(), to: b.clone() });

        // Outro app recriou o nome antigo DEPOIS do rename: o desfazer
        // tentaria voltar b→a e o rename do Windows APAGARIA o novo a.png.
        std::fs::write(&a, b"novo").unwrap();
        assert!(log.undo().is_err(), "destino ocupado tem que falhar");
        assert_eq!(std::fs::read(&a).unwrap(), b"novo", "o arquivo novo sobrevive");
        assert!(b.exists(), "o renomeado também sobrevive");
        assert!(!log.is_empty(), "a op volta pro log: nada foi desfeito");

        // Com o caminho livre de novo, o desfazer funciona.
        std::fs::remove_file(&a).unwrap();
        assert!(log.undo().unwrap().is_some());
        assert!(a.exists() && !b.exists());
        std::fs::remove_dir_all(&d).ok();
    }
}
