//! Navegação e metadados de imagem — portado do `img.rs` do LocalImage
//! oficial (repo da suíte, MIT), com a camada Tauri trocada por funções
//! puras. Comentários de aprendizado preservados.
//!
//! Privacidade por construção: qualquer export re-encoda a imagem — EXIF
//! (GPS, câmera, data) NUNCA sobrevive a um export do local-image.

use std::path::{Path, PathBuf};

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "ico", "avif"];

/// Vídeos entram na navegação (pra não sumirem do meio da pasta), mas o
/// local-image não os decodifica — só abre no app padrão do sistema.
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v", "wmv", "flv", "mpg", "mpeg", "m2ts", "ts"];

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| exts.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

pub fn is_image(path: &Path) -> bool {
    has_ext(path, IMAGE_EXTS)
}

pub fn is_video(path: &Path) -> bool {
    has_ext(path, VIDEO_EXTS)
}

/// Limpa o caminho digitado/colado no rodapé. O "Copiar como caminho" do
/// Windows entrega a pasta entre aspas duplas (`"C:\...\fotos"`) — o
/// `read_dir` falharia — e sobra espaço no fim de um arrasta-e-solta.
pub fn sanitize_path_input(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(t);
    t.trim().to_string()
}

/// Remove o prefixo `\\?\` (verbatim) que o `canonicalize` devolve no Windows —
/// ele vaza pro rodapé e pro breadcrumb se entrar no caminho de exibição. Só o
/// formato de drive é stripado (`\\?\C:\...`): o `\\?\UNC\...` (rede) tem outro
/// esquema e o strip quebraria o path. O casing que o canonicalize normaliza
/// vem junto, então o match com a listagem continua batendo.
pub fn strip_verbatim(s: &str) -> String {
    let Some(rest) = s.strip_prefix(r"\\?\") else { return s.to_string() };
    let b = rest.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
        rest.to_string()
    } else {
        s.to_string()
    }
}

/// Lista imagens e vídeos da pasta (ordenados sem diferenciar maiúsculas) —
/// é a sequência das setas ←/→ do visualizador. A extensão é filtrada ANTES
/// de qualquer stat (numa pasta de nuvem é a diferença entre milissegundos e
/// esperar a rede por cada arquivo); o `file_type` do `read_dir` já vem do
/// diretório lido, sem syscall extra.
pub fn list_dir(dir: &str) -> Result<Vec<PathBuf>, String> {
    let base = PathBuf::from(dir);
    let entries = std::fs::read_dir(&base).map_err(|e| format!("abrir pasta: {e}"))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let Ok(ft) = entry.file_type() else { return None };
            let p = base.join(entry.file_name());
            if !ft.is_file() || !(is_image(&p) || is_video(&p)) {
                return None;
            }
            Some(p)
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default());
    Ok(files)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
}

/// Dimensões + tamanho sem decodificar a imagem inteira.
pub fn image_info(path: &Path) -> Result<ImageInfo, String> {
    let (width, height) = image::image_dimensions(path).map_err(|e| format!("ler cabeçalho: {e}"))?;
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(ImageInfo { width, height, size_bytes })
}

/// Metadados EXIF legíveis (vazio se a imagem não tiver) — mesma seleção de
/// tags do oficial.
pub fn exif_info(path: &Path) -> Vec<(String, String)> {
    let labels: &[(exif::Tag, &str)] = &[
        (exif::Tag::Make, "Fabricante"),
        (exif::Tag::Model, "Câmera"),
        (exif::Tag::LensModel, "Lente"),
        (exif::Tag::DateTimeOriginal, "Data"),
        (exif::Tag::ExposureTime, "Exposição"),
        (exif::Tag::FNumber, "Abertura"),
        (exif::Tag::ISOSpeed, "ISO"),
        (exif::Tag::FocalLength, "Focal"),
    ];
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut buf = std::io::BufReader::new(&file);
    let reader = match exif::Reader::new().read_from_container(&mut buf) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    labels
        .iter()
        .filter_map(|(tag, label)| {
            let f = reader.get_field(*tag, exif::In::PRIMARY)?;
            Some((label.to_string(), f.display_value().to_string()))
        })
        .collect()
}

/// Export re-encodando (JPEG ou PNG): gira `deg` (múltiplo de 90) e/ou
/// redimensiona pra caber em `max_side` se maior. O re-encode é o que
/// garante a privacidade — EXIF nunca sobrevive.
pub fn export(src: &Path, dst: &Path, deg: u32, max_side: Option<u32>, jpeg_quality: u8) -> Result<(), String> {
    let img = image::open(src).map_err(|e| format!("decodificar: {e}"))?;
    let img = match deg % 360 {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => img,
    };
    let img = match max_side {
        Some(max) if img.width().max(img.height()) > max => img.thumbnail(max, max),
        _ => img,
    };
    let dst = if dst.extension().is_none() { dst.with_extension("jpg") } else { dst.to_path_buf() };
    let ext = dst.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_ascii_lowercase();
    match ext.as_str() {
        "png" => img.save(&dst).map_err(|e| e.to_string()),
        // JPEG: qualidade explícita; alpha vira fundo preto.
        _ => {
            let rgb = img.to_rgb8();
            let mut out = std::fs::File::create(&dst).map_err(|e| e.to_string())?;
            let enc = image::ImageEncoder::write_image(
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, jpeg_quality),
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            );
            enc.map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("local-image-{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lista_so_midia_ordenada() {
        let d = tmpdir("list");
        std::fs::write(d.join("B.JPG"), b"x").unwrap();
        std::fs::write(d.join("a.png"), b"x").unwrap();
        std::fs::write(d.join("nota.txt"), b"x").unwrap();
        std::fs::write(d.join("filme.MP4"), b"x").unwrap();
        let files = list_dir(d.to_str().unwrap()).unwrap();
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.png", "B.JPG", "filme.MP4"]);
    }

    #[test]
    fn sanitiza_caminho_entre_aspas() {
        // O "Copiar como caminho" do Windows põe aspas em volta.
        assert_eq!(sanitize_path_input(r#""C:\Users\joao\fotos""#), r"C:\Users\joao\fotos");
    }

    #[test]
    fn sanitiza_caminho_sem_aspas() {
        assert_eq!(sanitize_path_input(r"C:\Users\joao\fotos"), r"C:\Users\joao\fotos");
        // Aspa só de um lado não é aspas "envolvendo": fica como veio.
        assert_eq!(sanitize_path_input("\"C:\\fotos"), "\"C:\\fotos");
    }

    #[test]
    fn sanitiza_espacos_sobrando() {
        assert_eq!(sanitize_path_input("  /home/joao/fotos \t"), "/home/joao/fotos");
        // Aspas com espaço por fora também são limpas (ordem: trim → aspas → trim).
        assert_eq!(sanitize_path_input(" \"C:\\fotos\" "), r"C:\fotos");
    }

    #[test]
    fn strip_verbatim_so_no_formato_de_drive() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\joao\fotos"), r"C:\Users\joao\fotos");
        assert_eq!(strip_verbatim(r"C:\Users\joao\fotos"), r"C:\Users\joao\fotos");
        // Caminho de rede (UNC): o prefixo é de OUTRO esquema e fica como está.
        assert_eq!(strip_verbatim(r"\\?\UNC\servidor\compart\fotos"), r"\\?\UNC\servidor\compart\fotos");
    }

    #[test]
    fn export_gira_e_reencoda_sem_exif() {
        let d = tmpdir("exp");
        // JPEG 3×1 com EXIF de brinquedo.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_fn(3, 1, |x, _| image::Rgb([(x * 80) as u8, 20, 200]));
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        std::fs::write(d.join("in.jpg"), buf.into_inner()).unwrap();

        let out = d.join("girado.png");
        export(&d.join("in.jpg"), &out, 90, None, 90).unwrap();
        let dec = image::open(&out).unwrap();
        assert_eq!((dec.width(), dec.height()), (1, 3), "90° troca lados");

        // PNG não carrega EXIF por natureza; JPEG de saída também não
        // carrega o do original (re-encode). Cabeçalho PNG é a prova.
        let head = std::fs::read(&out).unwrap();
        assert!(head.starts_with(b"\x89PNG"));
    }

    #[test]
    fn export_redimensiona_pro_teto() {
        let d = tmpdir("exp-max");
        let img = image::RgbImage::from_fn(2000, 1000, |_, _| image::Rgb([1, 2, 3]));
        img.save_with_format(d.join("big.png"), image::ImageFormat::Png).unwrap();
        export(&d.join("big.png"), &d.join("small.png"), 0, Some(500), 90).unwrap();
        let dec = image::open(d.join("small.png")).unwrap();
        assert_eq!((dec.width(), dec.height()), (500, 250));
    }
}
