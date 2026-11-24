# ROADMAP — Local Utils

A light desktop app: one binary with small utilities, anchored by a clipboard manager that lives in
the tray. What has been delivered lives in the release notes; here stay the ideas, the open items
and the decisions that govern the app.

## Ideas

- **Snippets** — fixed text with placeholders (optional; must pass the admission criterion below).

## Open items for the Screenshot tab

- Capture history: today it keeps **only the pending one** — a new capture replaces the previous.
  A list of the session's shots (thumbnails + reopen in the editor) stays for later.
- Multi-DPI region selection: the crop is exact (mapping by ratio), but the overlay's visual
  alignment can come out crooked with monitors of different DPI.
- Options: the capture shortcut (`hotkey_screenshot`) still has no field in the options dialog —
  today only through `config.json`.

## Decisions

- Binaries embedded in the installer: evaluated and **rejected** (João, 2026-11-04) — every update
  would re-download ~80 MB.
- JXL encoder: discarded (AGPL license) — JXL enters only as a source (`jxl-oxide`).
- Temperature: best-effort — hwmon on Linux, WMI (`MSAcpi_ThermalZoneTemperature`) on Windows; the
  System tab's poll runs only with the tab visible.
- Out of scope: built-in sync, global hotkey on Linux, background monitoring.
- Clipboard listener: `AddClipboardFormatListener` on Windows (Linux is a stub today); planned:
  XFixes on X11 and data-control on Wayland.
- Global hotkey (opening the clipboard): crate `global-hotkey`, default `win+alt+v`.

## Admission criterion for a new utility

1. Real use ≥ 1x/week.
2. No mandatory network.
3. Does not duplicate a sibling app of the suite.
4. Opens instantly (no model to load, no index, no setup).
