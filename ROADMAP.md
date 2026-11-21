# ROADMAP — Local Utils

App desktop leve: um binário com utilitários pequenos, ancorado por um gerenciador
de clipboard residente na bandeja. O que já foi entregue mora nas release notes;
aqui ficam as ideias, as pendências e as decisões que governam o app.

## Ideias

- **Snippets** — texto fixo com placeholders (opcional; precisa passar no critério
  de admissão abaixo).

## Pendências da aba Screenshot

- Histórico de capturas: hoje guarda **só a pendente** — captura nova
  substitui a anterior. Uma lista de shots da sessão (miniaturas + reabrir no
  editor) fica pra depois.
- Seleção de região multi-DPI: o recorte é exato (mapeamento por razão), mas o
  alinhamento visual do overlay pode sair torto com monitores de DPI diferente.
- Opções: o atalho de captura (`hotkey_screenshot`) ainda não tem campo no
  diálogo de opções — hoje só pelo `config.json`.

## Decisões

- Binários embutidos no instalador: avaliado e **rejeitado** (João, 2026-11-04) —
  cada update re-baixaria ~80 MB.
- Encoder JXL: descartado (licença AGPL) — JXL entra só como origem (`jxl-oxide`).
- Temperatura: best-effort — hwmon no Linux, WMI (`MSAcpi_ThermalZoneTemperature`)
  no Windows; o poll da aba Sistema roda só com a aba visível.
- Fora de escopo: sync embutido, hotkey global no Linux, monitoramento em
  background.
- Listener do clipboard: `AddClipboardFormatListener` no Windows (hoje o Linux é
  stub); projetado: XFixes no X11 e data-control no Wayland.
- Hotkey global (abrir o clipboard): crate `global-hotkey`, padrão `win+alt+v`.

## Critério de admissão de utilitário novo

1. Uso real ≥ 1x/semana.
2. Sem rede obrigatória.
3. Não duplica app irmão da suíte.
4. Abre instantâneo (sem carregar modelo, sem índice, sem setup).
