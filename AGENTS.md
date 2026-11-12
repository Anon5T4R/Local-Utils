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
  zip, WiX v5 via `packaging/windows/package.ps1`) e Linux (`packaging/linux/package.sh`,
  molde do local-office: AppImage + tar.gz FHS **com `install.sh` na raiz** + deb + rpm),
  valida o PKGBUILD real (`packaging/arch/`, container Arch + `makepkg --printsrcinfo`)
  e publica. Mínimo local.
- **Disco local:** nunca deixe `target/` para trás — `cargo clean` após uso local;
  builds pesados são do CI.

## Estrutura
- `src/` — app (boot, abas, updater; `src/pickers.rs`: seletor nativo async via
  rfd + pollster, portal XDG no Linux)
- `utils-ui/` — tema, i18n e config comuns (`settings_ui`)
- `i18n/locales/<code>/localutils.tsv` — catálogos TSV (EN é a fonte)
- `icons/` — ícones do app (master PNG 1024)
- `assets/app-icon/hicolor/` — árvore de ícones commitada (gerada por `packaging/icons.sh`)
- `packaging/windows/package.ps1` + `local-utils.wxs` — MSI per-machine (WiX v5,
  Program Files\Local Utils) e portable zip
- `packaging/linux/` — `package.sh` (AppImage + tar FHS + install.sh + deb/rpm),
  `local-utils.desktop`, `local-utils.metainfo.xml.in`, `nfpm.yaml`, `postinst.sh`
- `packaging/arch/PKGBUILD` — pacote Arch a partir do tarball FHS da release

## Build
```
cargo build --locked                       # debug
cargo test --locked                        # testes
cargo xtask version X.Y.Z                  # bump (Cargo.toml + lock)
cargo xtask check                          # confere as versões sincronizadas
```


## Upstream e a pasta `<root>/storytold`

Menções a `storytold/*` em código, comentários, docs e atribuições (NOTICE/ATTRIBUTION) são referência histórica e obrigação de licença. **Nada** é lido de `<root>/storytold` (Windows `C:\Dev\storytold` · Arch `~/Dev/dev-arch/storytold`) em build, teste ou release: tudo vem do próprio repo (crates.io pinado pelo lock + `libs/`/`vendor/` vendored + CI que busca craft-fonts/craft-i18n do GitHub). Essa pasta existe somente como referência local para a skill `sync-upstream`.
