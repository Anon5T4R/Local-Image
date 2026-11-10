//! Seletor nativo que nunca bloqueia o frame.
//!
//! Molde: `local-pdf/crates/ui-egui/src/pickers.rs` (lá os comentários
//! explicam o porquê do `AsyncFileDialog`: modal síncrono re-entra no handler
//! do winit e aborta a app). Versão mínima dos pilotos: um seletor por vez,
//! resposta drenada no frame seguinte (`take` no início do `fn ui`, como o
//! `updater.tick`).
//!
//! Backends do rfd 0.17 (defaults): Windows = diálogo nativo; Linux = portal
//! XDG via zbus (Rust puro, fallback Zenity). macOS não é alvo dos pilotos;
//! se um dia for, criar o future na main thread antes de spawnar (ver
//! local-pdf).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Escolha pronta, consumida num frame posterior.
pub enum Picked {
    /// Pasta escolhida num `ask_folder`.
    Folder(PathBuf),
}

/// Seletor em andamento + escolhas prontas.
#[derive(Default)]
pub struct Pickers {
    showing: Arc<AtomicBool>,
    done: Arc<Mutex<Vec<Picked>>>,
}

impl Pickers {
    /// Um seletor está aberto (ou a resposta ainda espera o frame)?
    pub fn busy(&self) -> bool {
        self.showing.load(Ordering::SeqCst)
    }

    /// Pede uma pasta (`start` opcional = diretório inicial). A resposta sai
    /// em [`Self::take`] num frame posterior; cancelamento = silêncio.
    /// `false` = já há um seletor aberto (ou a thread não subiu).
    pub fn ask_folder(&self, ctx: &egui::Context, start: Option<&str>) -> bool {
        if self.busy() {
            return false;
        }
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(dir) = start.filter(|s| !s.is_empty()) {
            dialog = dialog.set_directory(dir);
        }
        self.showing.store(true, Ordering::SeqCst);
        let showing = self.showing.clone();
        let done = self.done.clone();
        let ctx = ctx.clone(); // Arc interno: barato; a thread exige 'static
        std::thread::Builder::new()
            .name("file-picker".into())
            .spawn(move || {
                if let Some(handle) = pollster::block_on(dialog.pick_folder()) {
                    done.lock().unwrap_or_else(PoisonError::into_inner)
                        .push(Picked::Folder(handle.path().to_path_buf()));
                }
                showing.store(false, Ordering::SeqCst);
                ctx.request_repaint(); // acorda o frame que consome a resposta
            })
            .is_ok()
    }

    /// Escolhas prontas — o frame que consome limpa a fila.
    pub fn take(&self) -> Vec<Picked> {
        std::mem::take(&mut *self.done.lock().unwrap_or_else(PoisonError::into_inner))
    }
}
