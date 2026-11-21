# Local Image

**Visualizador de imagens 100% local e offline.** Decode pela crate `image` →
textura egui: zoom na roda do mouse (ancorado no cursor), pan com arrastar,
painel EXIF e export re-encodado — **o EXIF (GPS, câmera, data) nunca
sobrevive a um export**; o export limpo grava `nome.clean.png` ao lado do
original. Gêmeo da suíte Local em egui/eframe
([Local-Player](https://github.com/Anon5T4R/Local-Player), Local-Keys...).

## Download

Asset mais recente nas
[Releases](https://github.com/Anon5T4R/Local-Image/releases):

| Asset | O que é |
| --- | --- |
| `local-image-<versão>-windows-x64.msi` | Instalador per-machine (WiX) |
| `local-image-<versão>-windows-x64-portable.zip` | Portable (um `.exe` só) |
| `local-image-<versão>-linux-x86_64.AppImage` | Linux, AppImage |
| `local-image-<versão>-linux-x86_64.deb` | Linux, Debian/Ubuntu |
| `local-image-<versão>-linux-x86_64.rpm` | Linux, Fedora/openSUSE |
| `local-image-<versão>-linux-x86_64.tar.gz` | Linux, portable FHS (+ Arch via PKGBUILD) |

## Atalhos

| Tecla | Ação |
| --- | --- |
| `←` / `→` | navega a pasta |
| `F` | fullscreen — ao **entrar** a imagem preenche a tela (cover); sair volta pro fit (Esc também sai) |
| `C` | centralizar mantendo o zoom |
| `I` | painel EXIF |
| `0` | encaixar na janela (fit; duplo-clique idem) |
| `R` | janela no tamanho da imagem |
| `[` / `]` | girar 90° anti-horário / horário |
| `Ctrl+K` / lupa | paleta de comandos; engrenagem: idioma, tema e Sobre |
| Clique na imagem | mostra / esconde a interface |

## Sem dependência externa

Nada é baixado em runtime: decode é da `image`, EXIF da `kamadak-exif` —
tudo embutido no binário.

## Build

```bash
cargo build --locked
cargo test --locked
cargo xtask ci    # o gate local: o mesmo que o CI roda
```

O `Cargo.lock` está commitado; `build.rs` embute o `icon.ico` no binário do
Windows.

Licença: MIT OR Apache-2.0 (`LICENSE-MIT` e `LICENSE-APACHE`).
