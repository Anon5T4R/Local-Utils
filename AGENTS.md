# AGENTS.md — Local Utils

Utilitários do cotidiano 100% locais e offline, parte da suíte Local do João.
Rust puro + egui, app single-crate com a UI comum em `utils-ui`. Sem conta, sem
telemetria, sem dependência de servidores de terceiros.

## Regras
- **Dependências:** toda dep que entra/sai/atualiza exige `Cargo.lock` commitado +
  linha no inventário do repo `Anon5T4R/local-deps` (regra completa em
  `local-deps/WORKFLOW.md`).
- **Sem fontes git:** nenhuma dependência pode apontar para `git+` (o CI valida).
  Tudo vem do crates.io (pinado pelo lock).
- **Commits:** português, sem Co-Authored-By.
- **Release:** tag `vX.Y.Z` → o Actions builda Windows (MSI per-machine + portable
  zip, WiX v5 via `packaging/windows/package.ps1`) e Linux (AppImage + tar.gz),
  valida o PKGBUILD do Arch e publica. Mínimo local.
- **Disco local:** nunca deixe `target/` para trás — `cargo clean` após uso local;
  builds pesados são do CI.

## Estrutura
- `src/` — app (boot, abas, updater)
- `utils-ui/` — tema, i18n e config comuns (`settings_ui`)
- `i18n/locales/<code>/localutils.tsv` — catálogos TSV (EN é a fonte)
- `icons/` — ícones do app
- `packaging/windows/package.ps1` + `local-utils.wxs` — MSI per-machine (WiX v5,
  Program Files\Local Utils) e portable zip
- `packaging/linux/local-utils.desktop` — desktop entry

## Build
```
cargo build --locked                       # debug
cargo test --locked                        # testes
cargo xtask version X.Y.Z                  # bump (Cargo.toml, utils-ui) + lock
cargo xtask check                          # confere as 2 versões sincronizadas
```


## Upstream e a pasta `C:\Dev\storytold`

Menções a `storytold/*` em código, comentários, docs e atribuições (NOTICE/ATTRIBUTION) são referência histórica e obrigação de licença. **Nada** é lido de `C:\Dev\storytold` em build, teste ou release: tudo vem do próprio repo (crates.io pinado pelo lock + `libs/`/`vendor/` vendored + CI que busca craft-fonts/craft-i18n do GitHub). Essa pasta existe somente como referência local para a skill `sync-upstream`.
