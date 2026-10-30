# Local Image

**Visualizador de imagens 100% local e offline.** Decode pela crate `image` →
textura egui: zoom na roda do mouse (ancorado no cursor), pan com arrastar,
painel EXIF e export re-encodado — **o EXIF (GPS, câmera, data) nunca
sobrevive a um export**. Gemeo da suíte Local em egui/eframe
([Local-Player](https://github.com/Anon5T4R/Local-Player), Local-Keys...).

## Download

Asset mais recente nas
[Releases](https://github.com/Anon5T4R/Local-Image/releases):

| Asset | O que é |
| --- | --- |
| `local-image-setup.exe` | Instalador NSIS (atalhos no Desktop + Menu Iniciar) |
| `local-image-windows-x64.zip` | Portable (um `.exe` só) |
| `local-image-x86_64.AppImage` | Linux, AppImage |
| `local-image-linux-x86_64.tar.gz` | Linux, portable |

## Atalhos

| Tecla | Ação |
| --- | --- |
| `←` / `→` | navega a pasta |
| `F` | fullscreen — ao **entrar** a imagem preenche a tela (cover); sair volta pro fit (Esc também sai) |
| `C` | centralizar mantendo o zoom |
| `0` | encaixar na janela (fit; duplo-clique idem) |
| `R` | janela no tamanho da imagem |
| `[` / `]` | girar 90° anti-horário / horário |
| Clique na imagem | mostra / esconde a interface |

## Sem dependência externa

Nada é baixado em runtime: decode é da `image`, EXIF da `kamadak-exif` —
tudo embutido no binário. Se um dia precisar de assets grandes, eles virão
das releases de
[Anon5T4R/big-files-local](https://github.com/Anon5T4R/big-files-local)
com **SHA-256** conferido na hora (mesmo desenho do mpv do Local-Player).

## Build

```bash
cargo build --release
```

O `Cargo.lock` está commitado; `build.rs` embute o `icon.ico` no binário do
Windows.

Licença: GPL-3.0-or-later.
