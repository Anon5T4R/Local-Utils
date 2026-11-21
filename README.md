# Local Utils

**Utilitários do cotidiano 100% locais e offline.** Um binário leve, ancorado por um
gerenciador de clipboard residente na bandeja; as demais ferramentas são abas
on-demand — só inicializam quando abertas. Irmão da suíte Local em egui/eframe
([Local-Keys](https://github.com/Anon5T4R/Local-Keys),
[Local-Player](https://github.com/Anon5T4R/Local-Player)...).

## Download

Asset mais recente nas
[Releases](https://github.com/Anon5T4R/Local-Utils/releases):

| Asset | O que é |
| --- | --- |
| `local-utils-<versão>-windows-x64.msi` | Instalador per-machine (atalhos no Desktop + Menu Iniciar) |
| `local-utils-<versão>-windows-x64-portable.zip` | Portable (um `.exe` só) |
| `local-utils-<versão>-linux-x86_64.AppImage` | Linux, AppImage |
| `local-utils-<versão>-linux-x86_64.tar.gz` | Linux, portable |

## Abas

| Aba | Papel |
| --- | --- |
| Clipboard | a âncora: histórico (texto/RTF/HTML/imagens/arquivos), busca, pin, retenção, export/import `.lutils` |
| Screenshot | captura de região com anotação mínima |
| Sistema | monitor de recursos/processos; temperaturas best-effort |
| Ferramentas | relógio, calculadora, unidades, dev tools |
| Scratchpad | nota rápida |
| Cor | conta-gotas de tela |
| Converter | conversão de imagens (PNG/JPEG/WebP; JXL só como origem) |

Snippets (texto fixo com placeholders) ainda é ideia — está no `ROADMAP.md`.

## Sem dependência externa

Nada é baixado em runtime e nada sai da máquina: sem conta, sem telemetria, sem
servidor de terceiros. Atualização é o único acesso à rede, e só quando você pede
(ou no check de arranque) — o download vem das releases do GitHub com SHA-256
conferido pelo `craft_update`.

## Build

```bash
cargo build --locked
```

O `Cargo.lock` está commitado; `build.rs` embute o `icon.ico` no binário do
Windows.

Licença: MIT OR Apache-2.0 (`LICENSE-MIT` e `LICENSE-APACHE`).
