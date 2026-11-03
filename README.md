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

## Módulos

| Versão | Módulo | Papel |
| --- | --- | --- |
| v0.1 | Clipboard | A âncora: histórico, busca, pin, retenção, export/import `.lutils` |
| v0.2 | Clipboard | Popup rápido no cursor + colar direto + OCR nativo |
| v0.3 | Screenshot | Captura de região com anotação mínima |
| v0.4 | Tools + Scratchpad | Relógio, calculadora, conversor, dev tools; nota rápida |
| v0.5 | Sistema + Color picker | Monitor de recursos/processos; conta-gotas de tela |
| v0.6+ | Snippets | Texto fixo com placeholders (opcional) |

O detalhamento de cada módulo está no `ROADMAP.md`; o plano completo vive em
`docs/planos/localutils.md` do repo de notas.

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

Licença: GPL-3.0-or-later.
