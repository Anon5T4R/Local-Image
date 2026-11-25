# AGENTS.md — LocalImage

Visualizador de imagens 100% local e offline, parte da suíte Local do João.
Rust puro + egui, app single-crate. Sem conta, sem telemetria, sem dependência de servidores de terceiros.

## Regras
- **Dependências:** toda dep que entra/sai/atualiza exige `Cargo.lock` commitado +
  linha no inventário do repo `Styx-Studios/local-deps` (regra completa em
  `local-deps/WORKFLOW.md`).
- **Sem fontes git:** nenhuma dependência pode apontar para `git+` (o CI valida).
  Tudo vem do crates.io (pinado pelo lock).
- **Commits:** português, sem Co-Authored-By.
- **Release:** tag `vX.Y.Z` via `release.sh`; fluxo, gate local e checklist no
  `docs/git-protocol.md` do dev-notes.
- **Disco:** no workspace do João vale a política de `target/` do dev-notes (`MEMORY.md` §4 — limpa o que
  está frio ou feito, mantém durante iteração ativa); fora dele, `cargo clean` ao terminar. Builds pesados são do CI.

## Estrutura
- `src/` — app single-crate (`src/pickers.rs`: seletor nativo async, molde do local-pdf;
  também `src/i18n.rs`, `src/prefs.rs`, `src/theme.rs`, `src/shell.rs` e
  `i18n/locales/<code>/*.tsv`)
- Casca da suíte = `src/shell.rs` + craft-icons + craft-palette + craft-desktop-theme
  (espelhos do local-libs)
- `icons/` — ícones do app (master PNG 1024)
- `assets/app-icon/hicolor/` — árvore de ícones commitada (gerada por `packaging/icons.sh`)
- `packaging/windows/package.ps1` + `local-image.wxs` — MSI per-machine (WiX v5, Program
  Files\Local Image) e portable zip
- `packaging/linux/` — `package.sh` (AppImage + tar FHS + install.sh + deb/rpm),
  `local-image.desktop`, `local-image.metainfo.xml.in`, `nfpm.yaml`, `postinst.sh`
- `packaging/arch/PKGBUILD` — pacote Arch a partir do tarball FHS da release

## Build
```
cargo build --locked                       # debug
cargo test --locked                        # testes
cargo xtask ci                             # o gate local: o mesmo que o CI (fmt, clippy, build, testes, cargo-deny,
                                           # espelhos do local-libs)
cargo xtask version set X.Y.Z              # bump (Cargo.toml) + lock
cargo xtask check                          # confere a versão do Cargo.toml
```


## Agent-native (MCP)
Todo comando do app é alcançável por um agente. O servidor é `local-image mcp` (stdio), sobre o
`craft-mcp` da suíte (`libs/craft-libs/crates/mcp`, fonte `local-libs` — espelho, não editar), com
três ferramentas: `command_list` (o catálogo: id, rótulo, menu, `appOnly` e o schema dos parâmetros),
`command_run` (um id com `params`) e `command_batch` (passos em ordem, com `"$N.path"` puxando o
resultado de um passo anterior). Sem cliente MCP: `local-image commands [--search TEXT]` imprime o
catálogo e `local-image run --script job.json` roda os mesmos passos. O catálogo é
`src/automation/catalogue.rs` — os comandos da paleta mais `file.open` e `image.info`, que o app já
tinha fora dela (o caminho do "Abrir com"/args e os números do rodapé). Nada de ação nova.

Os comandos são os mesmos no headless e na janela (`src/automation/session.rs`, uma implementação
para os dois). O que precisa da janela (viewport, diálogos, updater, sair) diz `appOnly` e o
servidor headless o recusa apontando o modo live.

Modo live: `local-image --control PORT` abre o canal JSON em `127.0.0.1` e
`local-image mcp --connect PORT` liga o MCP nele (porta nua = loopback; host que não é loopback é
recusado antes de conectar). `--automation-read-root DIR`/`--automation-write-root DIR` (repetíveis,
`--flag=DIR`) confinam os arquivos que os comandos leem e escrevem; uma espécie sem raiz não concede
nada daquele acesso e, sem nenhuma flag, não há confinamento (o app do usuário). Com um app rodando,
as flags vão para o app (`local-image --control PORT --automation-read-root DIR …`). As três vias
têm teste em `tests/agent_native.rs`. O binário no Windows é subsistema GUI: `commands`/`run` devem
ter a saída redirecionada (`> out.json`) ou vir de um cliente MCP, que já os canaliza.

## Upstream e a pasta `<root>/storytold`

Menções a `storytold/*` em código, docs e atribuições (NOTICE/ATTRIBUTION) são referência histórica e
obrigação de licença — nada é lido dessa pasta em build, teste ou release.
