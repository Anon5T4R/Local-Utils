# ROADMAP — Local Utils

App desktop leve: um binário com utilitários pequenos, ancorado por um gerenciador
de clipboard residente na bandeja. Plano completo em `docs/planos/localutils.md`.

## Fatias

| Versão | Entrega |
| --- | --- |
| v0.1 | Tray + listener + histórico completo (texto/RTF/HTML/imagens/arquivos) + busca/filtro/pin/retenção + teclado herdado + esconder-ao-copiar + editar snippet + recopiar + QR + exclusão de apps sensíveis (lista editável) + export/import `.lutils` + autostart + hotkey global abrindo a janela principal + settings + MSI/CI/autoupdate |
| v0.2 | Popup rápido no cursor + colar direto na janela focada (SendInput) + OCR nativo |
| v0.3 | Screenshot com editor de anotação mínimo |
| v0.4 | Tools (relógio, calculadora, conversor, dev tools) + scratchpad |
| v0.5 | Sistema (monitor + temperaturas best-effort) + color picker |
| v0.6+ | Snippets com placeholders (opcional) |

## Abas do esqueleto (v0.0 do scaffold)

| Aba | Versão de entrada |
| --- | --- |
| Clipboard | v0.1 (âncora, padrão ao abrir) |
| Screenshot | v0.3 |
| Sistema | v0.5 |
| Tools | v0.4 |
| Scratchpad | v0.4 |

## Critério de admissão de utilitário novo

1. Uso real ≥ 1x/semana.
2. Sem rede obrigatória.
3. Não duplica app irmão da suíte.
4. Abre instantâneo (sem carregar modelo, sem índice, sem setup).
