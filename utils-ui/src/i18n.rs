//! i18n via craft-i18n (padrão storytold): catálogos TSV em `i18n/locales/` embutidos
//! pelo `build.rs` (fonte: `scripts/gen_i18n.py` converte este enum). O enum `Key`
//! fica como API tipada (compilador cobre completude); o lookup é o motor do
//! craft-i18n, com fallback `pt-br → pt → en` e ausência de tradução = inglês.

use std::sync::LazyLock;

use craft_i18n::I18n;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// Códigos dos idiomas oferecidos (mesmos do enum `Lang`).
const OFFERED: &[&str] = &["en", "pt-br", "es"];

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, OFFERED);
    for p in problems {
        eprintln!("utils-ui i18n: {p}");
    }
    i18n
});

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    Pt,
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Pt, Lang::En, Lang::Es];

    /// Nome no seletor — sempre endônimo (regra da suíte).
    pub fn label(self) -> &'static str {
        match self {
            Lang::Pt => "Português",
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    // compartilhadas
    AppTitle,
    Language,
    Theme,
    // valores do seletor de tema (o `Appearance::label` é só o código)
    AppearanceSystem,
    AppearanceLight,
    AppearanceDark,
    ForgetKey,
    // verificação de atualização (o prompt é o dialog do craft_update)
    CheckUpdates,
    UpToDate,
    // abas do app
    TabClipboard,
    TabScreenshot,
    TabSystem,
    TabTools,
    TabScratchpad,
    // aviso do esqueleto: a aba ainda não tem conteúdo
    TabWip,
    EmptyList,
    NoResults,
    ItemOne,
    // rodapé: quantas cópias o histórico guarda hoje
    ItemsCount,
    // aba Clipboard: busca e filtro por tipo
    Search,
    FilterAll,
    FilterText,
    FilterImages,
    FilterFiles,
    FilterRich,
    // aba Clipboard: ações da entrada
    CopyAgain,
    Edit,
    Save,
    Cancel,
    Remove,
    QRCode,
    CopiedOk,
    CopyFailed,
    Pinned,
    Unpinned,
    // ação do ★ da linha (fixar/desafixar — o texto do estado fica no rodapé)
    Pin,
    Unpin,
    // limpar o histórico: botão da barra de filtros, confirmação e resultado
    ClearHistory,
    ClearAll,
    ClearExplain,
    HistoryCleared,
    // aba Clipboard: etiqueta do tipo na linha
    TypeText,
    TypeHtml,
    TypeRtf,
    TypeImage,
    TypeFiles,
    // aba Clipboard: carimbo relativo da entrada
    TsNow,
    TsTime,
    TsYesterday,
    TsDate,
    // aba Clipboard: o que impede um QR (texto longo demais / falha ao gerar)
    QrTooLong,
    QrFailed,
    // opções (engrenagem): autostart, atalho global, exclusões e o `.lutils`
    Options,
    Autostart,
    HotkeyGlobal,
    // captura do atalho (opções): botão e o "pressione as teclas" do campo
    RegisterHotkey,
    PressKeysHint,
    HotkeyConflict,
    HotkeyInvalid,
    HotkeyUnavailable,
    ExcludeApps,
    ExcludeHint,
    ExcludeAdd,
    RestartApplies,
    ExportHistory,
    ImportHistory,
    Exported,
    Imported,
    ExportFailed,
    ImportFailed,
    WindowsOnly,
    AutostartFailed,
    Close,
    // opções: o atalho abre o popup no cursor (em vez da janela inteira)
    TogglePopupMode,
    // bandeja: menu do ícone
    TrayOpen,
    TrayQuit,
    // popup rápido: busca sem resultado
    PopupNoResults,
    // colar direto na janela em foco (o popup manda Ctrl+V)
    PasteFailed,
    // OCR das imagens do histórico
    OcrButton,
    OcrReading,
    OcrEmpty,
    OcrFailed,
    CopyText,
    // captura de tela (aba Screenshot): dica do atalho e falha da captura
    CaptureHint,
    CaptureFailed,
    // editor da captura: ferramentas da barra
    ToolSelect,
    ToolRect,
    ToolArrow,
    ToolText,
    ToolPixelate,
    ToolBlur,
    ToolCrop,
    // editor da captura: ações e rótulos
    Undo,
    ClearAnnotations,
    CropReset,
    CopyShot,
    SaveShot,
    Color,
    Thickness,
    Strength,
    ShotText,
    ShotSaved,
    ShotSaveFailed,
    // aba Ferramentas: sub-abas (Relógio, Calculadora, Unidades, Dev)
    ToolsClock,
    CalcTab,
    UnitsTab,
    DevTab,
    // aba Ferramentas › relógio: timer, pomodoro, cronômetro e alarmes
    ClockTimer,
    ClockPomodoro,
    ClockStopwatch,
    ClockAlarms,
    ClockStart,
    ClockPause,
    ClockStop,
    ClockReset,
    ClockTimerHint,
    ClockTimeInvalid,
    ClockTimeUp,
    ClockPomodoroWork,
    ClockPomodoroBreak,
    ClockPomodoroWorkMin,
    ClockPomodoroBreakMin,
    ClockPomodoroCycles,
    ClockAlarmTime,
    ClockAlarmLabel,
    ClockAlarmInvalid,
    ClockAlarmRinging,
    ClockAlarmsEmpty,
    // aba Ferramentas › calculadora
    CalcExpression,
    CalcHistory,
    CalcEmpty,
    CalcSyntax,
    CalcUnbalanced,
    CalcDivZero,
    CalcUnknownName,
    CalcBadNumber,
    CalcDomain,
    // aba Ferramentas › conversor de unidades
    UnitsLength,
    UnitsMass,
    UnitsTemperature,
    UnitsData,
    UnitsSpeed,
    UnitsValue,
    UnitsFrom,
    UnitsTo,
    UnitsInvalid,
    // aba Ferramentas › dev (Base64, URL, hash, UUID, timestamp)
    DevBase64,
    DevUrl,
    DevHash,
    DevUuid,
    DevUuidNote,
    DevTimestamp,
    DevInput,
    DevEncode,
    DevDecode,
    DevBadInput,
    DevBadUtf8,
    DevBadIso,
    DevIsoHint,
    DevEpoch,
    DevToEpoch,
    DevToIso,
    DevNow,
    DevGenerate,
    // aba Notas (scratchpad)
    NotesHint,
    NotesChars,
    NotesLines,
    NotesSaved,
    // copiar um resultado de ferramenta
    CopyValue,
    // aba Cor: seletor (hex/RGB/HSL), conta-gotas e recentes
    TabPicker,
    CaptureColor,
    PickerHint,
    RecentColors,
    InvalidColor,
    LabelHex,
    LabelRgb,
    LabelHsl,
    // aba Converter: zona de soltar, fila e conversão
    TabConvert,
    DropImagesHint,
    SelectFiles,
    Quality,
    QualityHint,
    ConvertedTo,
    ConvertFailed,
    UnsupportedType,
    ConvertWaiting,
    ClearDone,
    JxlInputOnly,
}

/// Todas as chaves — usado pelo teste de completude (fora de teste o match
/// exaustivo do `t()` já cobre; o const fica silenciado pra não warnar na
/// build da lib).
#[cfg_attr(not(test), allow(dead_code))]
const KEYS: &[Key] = &[
    Key::AppTitle,
    Key::Language,
    Key::Theme,
    Key::AppearanceSystem,
    Key::AppearanceLight,
    Key::AppearanceDark,
    Key::ForgetKey,
    Key::CheckUpdates,
    Key::UpToDate,
    Key::TabClipboard,
    Key::TabScreenshot,
    Key::TabSystem,
    Key::TabTools,
    Key::TabScratchpad,
    Key::TabWip,
    Key::EmptyList,
    Key::NoResults,
    Key::ItemOne,
    Key::ItemsCount,
    Key::Search,
    Key::FilterAll,
    Key::FilterText,
    Key::FilterImages,
    Key::FilterFiles,
    Key::FilterRich,
    Key::CopyAgain,
    Key::Edit,
    Key::Save,
    Key::Cancel,
    Key::Remove,
    Key::QRCode,
    Key::CopiedOk,
    Key::CopyFailed,
    Key::Pinned,
    Key::Unpinned,
    Key::Pin,
    Key::Unpin,
    Key::ClearHistory,
    Key::ClearAll,
    Key::ClearExplain,
    Key::HistoryCleared,
    Key::TypeText,
    Key::TypeHtml,
    Key::TypeRtf,
    Key::TypeImage,
    Key::TypeFiles,
    Key::TsNow,
    Key::TsTime,
    Key::TsYesterday,
    Key::TsDate,
    Key::QrTooLong,
    Key::QrFailed,
    Key::Options,
    Key::Autostart,
    Key::HotkeyGlobal,
    Key::RegisterHotkey,
    Key::PressKeysHint,
    Key::HotkeyConflict,
    Key::HotkeyInvalid,
    Key::HotkeyUnavailable,
    Key::ExcludeApps,
    Key::ExcludeHint,
    Key::ExcludeAdd,
    Key::RestartApplies,
    Key::ExportHistory,
    Key::ImportHistory,
    Key::Exported,
    Key::Imported,
    Key::ExportFailed,
    Key::ImportFailed,
    Key::WindowsOnly,
    Key::AutostartFailed,
    Key::Close,
    Key::TogglePopupMode,
    Key::TrayOpen,
    Key::TrayQuit,
    Key::PopupNoResults,
    Key::PasteFailed,
    Key::OcrButton,
    Key::OcrReading,
    Key::OcrEmpty,
    Key::OcrFailed,
    Key::CopyText,
    Key::CaptureHint,
    Key::CaptureFailed,
    Key::ToolSelect,
    Key::ToolRect,
    Key::ToolArrow,
    Key::ToolText,
    Key::ToolPixelate,
    Key::ToolBlur,
    Key::ToolCrop,
    Key::Undo,
    Key::ClearAnnotations,
    Key::CropReset,
    Key::CopyShot,
    Key::SaveShot,
    Key::Color,
    Key::Thickness,
    Key::Strength,
    Key::ShotText,
    Key::ShotSaved,
    Key::ShotSaveFailed,
    Key::ToolsClock,
    Key::CalcTab,
    Key::UnitsTab,
    Key::DevTab,
    Key::ClockTimer,
    Key::ClockPomodoro,
    Key::ClockStopwatch,
    Key::ClockAlarms,
    Key::ClockStart,
    Key::ClockPause,
    Key::ClockStop,
    Key::ClockReset,
    Key::ClockTimerHint,
    Key::ClockTimeInvalid,
    Key::ClockTimeUp,
    Key::ClockPomodoroWork,
    Key::ClockPomodoroBreak,
    Key::ClockPomodoroWorkMin,
    Key::ClockPomodoroBreakMin,
    Key::ClockPomodoroCycles,
    Key::ClockAlarmTime,
    Key::ClockAlarmLabel,
    Key::ClockAlarmInvalid,
    Key::ClockAlarmRinging,
    Key::ClockAlarmsEmpty,
    Key::CalcExpression,
    Key::CalcHistory,
    Key::CalcEmpty,
    Key::CalcSyntax,
    Key::CalcUnbalanced,
    Key::CalcDivZero,
    Key::CalcUnknownName,
    Key::CalcBadNumber,
    Key::CalcDomain,
    Key::UnitsLength,
    Key::UnitsMass,
    Key::UnitsTemperature,
    Key::UnitsData,
    Key::UnitsSpeed,
    Key::UnitsValue,
    Key::UnitsFrom,
    Key::UnitsTo,
    Key::UnitsInvalid,
    Key::DevBase64,
    Key::DevUrl,
    Key::DevHash,
    Key::DevUuid,
    Key::DevUuidNote,
    Key::DevTimestamp,
    Key::DevInput,
    Key::DevEncode,
    Key::DevDecode,
    Key::DevBadInput,
    Key::DevBadUtf8,
    Key::DevBadIso,
    Key::DevIsoHint,
    Key::DevEpoch,
    Key::DevToEpoch,
    Key::DevToIso,
    Key::DevNow,
    Key::DevGenerate,
    Key::NotesHint,
    Key::NotesChars,
    Key::NotesLines,
    Key::NotesSaved,
    Key::CopyValue,
    Key::TabPicker,
    Key::CaptureColor,
    Key::PickerHint,
    Key::RecentColors,
    Key::InvalidColor,
    Key::LabelHex,
    Key::LabelRgb,
    Key::LabelHsl,
    Key::TabConvert,
    Key::DropImagesHint,
    Key::SelectFiles,
    Key::Quality,
    Key::QualityHint,
    Key::ConvertedTo,
    Key::ConvertFailed,
    Key::UnsupportedType,
    Key::ConvertWaiting,
    Key::ClearDone,
    Key::JxlInputOnly,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    let code = match lang {
        Lang::Pt => "pt-br",
        Lang::En => "en",
        Lang::Es => "es",
    };
    // A fonte inglesa é a chave de lookup do catálogo (padrão craft-i18n).
    let source = match key {
        Key::AppTitle => "Local Utils",
        Key::Language => "Language",
        Key::Theme => "Theme",
        Key::AppearanceSystem => "System theme",
        Key::AppearanceLight => "Light",
        Key::AppearanceDark => "Dark",
        Key::ForgetKey => "Forget key",
        Key::CheckUpdates => "Check for updates",
        Key::UpToDate => "up to date",
        Key::TabClipboard => "Clipboard",
        Key::TabScreenshot => "Screenshot",
        Key::TabSystem => "System",
        Key::TabTools => "Tools",
        Key::TabScratchpad => "Scratchpad",
        Key::TabWip => "Under construction",
        Key::EmptyList => "Nothing copied yet",
        Key::NoResults => "Nothing found",
        Key::ItemOne => "{n} item",
        // `{n}` é preenchido pelo chamador (contagem do histórico no rodapé).
        Key::ItemsCount => "{n} items",
        Key::Search => "Search",
        Key::FilterAll => "All",
        Key::FilterText => "Text",
        Key::FilterImages => "Images",
        Key::FilterFiles => "Files",
        Key::FilterRich => "Rich",
        Key::CopyAgain => "Copy again",
        Key::Edit => "Edit",
        Key::Save => "Save",
        Key::Cancel => "Cancel",
        Key::Remove => "Remove",
        Key::QRCode => "QR code",
        Key::CopiedOk => "Copied ✓",
        Key::CopyFailed => "Could not copy",
        Key::Pinned => "Pinned",
        Key::Unpinned => "Unpinned",
        Key::Pin => "Pin",
        Key::Unpin => "Unpin",
        Key::ClearHistory => "Clear",
        Key::ClearAll => "Clear all",
        Key::ClearExplain => "Pinned items are kept",
        Key::HistoryCleared => "history cleared",
        Key::TypeText => "Plain text",
        Key::TypeHtml => "HTML",
        Key::TypeRtf => "RTF",
        Key::TypeImage => "Image",
        Key::TypeFiles => "Copied files",
        Key::TsNow => "now",
        // `{h}` e `{mm}` são preenchidos pelo chamador (hora local).
        Key::TsTime => "{h}:{mm}",
        Key::TsYesterday => "yesterday",
        // `{d}` e `{month}` são preenchidos pelo chamador (dd/mm).
        Key::TsDate => "{d}/{month}",
        Key::QrTooLong => "Text too long for a QR code (~1000 characters)",
        Key::QrFailed => "Could not generate the QR code",
        Key::Options => "Options",
        Key::Autostart => "Start with Windows",
        Key::HotkeyGlobal => "Global hotkey",
        Key::RegisterHotkey => "Register shortcut",
        Key::PressKeysHint => "Press the new shortcut… (Esc cancels)",
        Key::HotkeyConflict => "shortcut already in use",
        Key::HotkeyInvalid => "invalid shortcut — using the default",
        Key::HotkeyUnavailable => "could not register the shortcut",
        Key::ExcludeApps => "Sensitive apps",
        Key::ExcludeHint => "Executable name (e.g. keepass.exe)",
        Key::ExcludeAdd => "Add",
        Key::RestartApplies => "Applies after restarting the app",
        Key::ExportHistory => "Export",
        Key::ImportHistory => "Import",
        Key::Exported => "history exported",
        Key::Imported => "history imported",
        Key::ExportFailed => "Could not export the history",
        Key::ImportFailed => "Could not import the history",
        Key::WindowsOnly => "Available on Windows only",
        Key::AutostartFailed => "Could not change the autostart",
        Key::Close => "Close",
        Key::TogglePopupMode => "Open a quick popup at the cursor",
        Key::TrayOpen => "Open Local Utils",
        Key::TrayQuit => "Quit",
        Key::PopupNoResults => "Nothing found",
        Key::PasteFailed => "Could not paste into the active window",
        Key::OcrButton => "OCR",
        Key::OcrReading => "reading the image…",
        Key::OcrEmpty => "no text found in the image",
        Key::OcrFailed => "Could not read the image",
        Key::CopyText => "Copy text",
        // `{hotkey}` é preenchido pelo chamador (o atalho do config, legível).
        Key::CaptureHint => "Press {hotkey} to capture",
        Key::CaptureFailed => "Could not capture the screen",
        Key::ToolSelect => "Select",
        Key::ToolRect => "Rectangle",
        Key::ToolArrow => "Arrow",
        // Reaproveita a linha "Text" do catálogo (a mesma do filtro da aba
        // Clipboard): uma chave por STRING, não por enum.
        Key::ToolText => "Text",
        Key::ToolPixelate => "Pixelate",
        Key::ToolBlur => "Blur",
        Key::ToolCrop => "Crop",
        Key::Undo => "Undo",
        Key::ClearAnnotations => "Clear annotations",
        Key::CropReset => "Reset crop",
        Key::CopyShot => "Copy image",
        Key::SaveShot => "Save as PNG",
        Key::Color => "Color",
        Key::Thickness => "Thickness",
        Key::Strength => "Strength",
        Key::ShotText => "Text to insert",
        Key::ShotSaved => "screenshot saved",
        Key::ShotSaveFailed => "Could not save the screenshot",
        Key::ToolsClock => "Clock",
        Key::CalcTab => "Calculator",
        Key::UnitsTab => "Units",
        Key::DevTab => "Dev tools",
        Key::ClockTimer => "Timer",
        Key::ClockPomodoro => "Pomodoro",
        Key::ClockStopwatch => "Stopwatch",
        Key::ClockAlarms => "Alarms",
        Key::ClockStart => "Start",
        Key::ClockPause => "Pause",
        Key::ClockStop => "Stop",
        Key::ClockReset => "Reset",
        Key::ClockTimerHint => "Time (hh:mm:ss)",
        Key::ClockTimeInvalid => "invalid time — use hh:mm:ss",
        Key::ClockTimeUp => "time's up",
        Key::ClockPomodoroWork => "Work",
        Key::ClockPomodoroBreak => "Break",
        Key::ClockPomodoroWorkMin => "Work (min)",
        Key::ClockPomodoroBreakMin => "Break (min)",
        // `{n}` é preenchido pelo chamador (ciclos de trabalho do pomodoro).
        Key::ClockPomodoroCycles => "{n} cycles",
        Key::ClockAlarmTime => "Alarm (hh:mm)",
        Key::ClockAlarmLabel => "Label",
        Key::ClockAlarmInvalid => "invalid time — use hh:mm",
        Key::ClockAlarmRinging => "alarm: {label}",
        Key::ClockAlarmsEmpty => "No alarms yet",
        Key::CalcExpression => "Expression",
        Key::CalcHistory => "History",
        Key::CalcEmpty => "No calculations yet",
        Key::CalcSyntax => "syntax error",
        Key::CalcUnbalanced => "missing parenthesis",
        Key::CalcDivZero => "division by zero",
        Key::CalcUnknownName => "unknown name: {name}",
        Key::CalcBadNumber => "invalid number",
        Key::CalcDomain => "invalid operation",
        Key::UnitsLength => "Length",
        Key::UnitsMass => "Mass",
        Key::UnitsTemperature => "Temperature",
        Key::UnitsData => "Data",
        Key::UnitsSpeed => "Speed",
        Key::UnitsValue => "Value",
        Key::UnitsFrom => "From",
        Key::UnitsTo => "To",
        Key::UnitsInvalid => "enter a number",
        Key::DevBase64 => "Base64",
        Key::DevUrl => "URL",
        Key::DevHash => "Hash",
        Key::DevUuid => "UUID v4",
        Key::DevUuidNote => "unique, but not cryptographic",
        Key::DevTimestamp => "Timestamp",
        Key::DevInput => "Input",
        Key::DevEncode => "Encode",
        Key::DevDecode => "Decode",
        Key::DevBadInput => "invalid input",
        Key::DevBadUtf8 => "not valid UTF-8",
        Key::DevBadIso => "invalid date",
        Key::DevIsoHint => "ISO date (e.g. 2026-11-04T14:30:00)",
        Key::DevEpoch => "Unix epoch (seconds)",
        Key::DevToEpoch => "To epoch",
        Key::DevToIso => "To ISO",
        Key::DevNow => "Now",
        Key::DevGenerate => "Generate",
        Key::NotesHint => "Type anything — it saves on its own",
        Key::NotesChars => "{n} characters",
        Key::NotesLines => "{n} lines",
        Key::NotesSaved => "saved ✓",
        Key::CopyValue => "Copy",
        // Aba Cor: "Color" reaproveita a linha do editor da captura (a chave do
        // catálogo é a STRING, não o enum).
        Key::TabPicker => "Color",
        Key::CaptureColor => "Capture from screen",
        Key::PickerHint => "Click to pick a color · Esc cancels",
        Key::RecentColors => "Recent colors",
        Key::InvalidColor => "invalid color",
        Key::LabelHex => "Hex",
        Key::LabelRgb => "RGB",
        Key::LabelHsl => "HSL",
        // Aba Converter: o rótulo da aba e o botão de disparar são a MESMA
        // palavra (uma linha só no catálogo, como o "Text" das Ferramentas).
        Key::TabConvert => "Convert",
        Key::DropImagesHint => "Drop images here",
        Key::SelectFiles => "Select files",
        Key::Quality => "Quality",
        Key::QualityHint => "Applies to JPEG; WebP is saved lossless",
        // `{name}` é preenchido pelo chamador (arquivo de saída).
        Key::ConvertedTo => "saved as {name}",
        Key::ConvertFailed => "Could not convert",
        Key::UnsupportedType => "not a supported image",
        Key::ConvertWaiting => "waiting",
        Key::ClearDone => "Clear finished",
        Key::JxlInputOnly => "JXL is input only (no JXL encoder yet)",
    };
    let craft_lang = I18N.from_code(code).unwrap_or_else(|| I18N.from_code("en").expect("en é builtin"));
    I18N.t(craft_lang, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equivalente do "tsc força completude": toda chave tem tradução nos
    /// 3 idiomas (match exaustivo + não-vazio).
    #[test]
    fn todas_as_chaves_tem_traducao() {
        for lang in Lang::ALL {
            for key in KEYS {
                let s = t(lang, *key);
                assert!(!s.is_empty(), "chave {key:?} vazia em {lang:?}");
            }
        }
    }

    /// O motor craft-i18n está ligado: pt-br sai do catálogo TSV, não do inglês.
    #[test]
    fn craft_i18n_busca_no_catalogo() {
        assert_eq!(t(Lang::Pt, Key::TabWip), "Em construção");
        assert_eq!(t(Lang::Es, Key::TabWip), "En construcción");
        assert_eq!(t(Lang::En, Key::TabWip), "Under construction");
        assert_eq!(t(Lang::Pt, Key::CheckUpdates), "Verificar atualizações");
        assert_eq!(t(Lang::Es, Key::CheckUpdates), "Buscar actualizaciones");
        // Marca não traduz.
        assert_eq!(t(Lang::Pt, Key::AppTitle), "Local Utils");
    }

    /// O container `right_to_left` da barra de topo e o rodapé dependem destas
    /// linhas: filtro "All" e a contagem "{n} items" já pagaram QA em inglês
    /// quando a linha do TSV faltava — o teste trava as duas no catálogo.
    #[test]
    fn contagem_e_filtro_saem_do_catalogo() {
        assert_eq!(t(Lang::Pt, Key::FilterAll), "Tudo");
        assert_eq!(t(Lang::Es, Key::FilterAll), "Todo");
        assert_eq!(t(Lang::En, Key::FilterAll), "All");

        assert_eq!(t(Lang::Pt, Key::ItemsCount), "{n} itens");
        assert_eq!(t(Lang::Es, Key::ItemsCount), "{n} elementos");
        assert_eq!(t(Lang::Pt, Key::ItemOne), "{n} item");
        assert_eq!(t(Lang::En, Key::ItemsCount).replace("{n}", "4"), "4 items");
        assert_eq!(t(Lang::Pt, Key::ItemsCount).replace("{n}", "4"), "4 itens");
    }

    /// O seletor de tema não mostra mais o valor em inglês em pt/es.
    #[test]
    fn valores_do_tema_traduzem() {
        assert_eq!(t(Lang::Pt, Key::AppearanceSystem), "Sistema");
        assert_eq!(t(Lang::Pt, Key::AppearanceLight), "Claro");
        assert_eq!(t(Lang::Pt, Key::AppearanceDark), "Escuro");
        assert_eq!(t(Lang::Es, Key::AppearanceSystem), "Sistema");
        assert_eq!(t(Lang::Es, Key::AppearanceDark), "Oscuro");
    }

    /// As chaves do v0.1.1 (limpar, pin e captura do atalho) têm linha nos
    /// TSVs: sem elas a UI cairia no inglês sem nenhum aviso.
    #[test]
    fn chaves_novas_saem_do_catalogo() {
        assert_eq!(t(Lang::Pt, Key::Pin), "Fixar");
        assert_eq!(t(Lang::Pt, Key::Unpin), "Desafixar");
        assert_eq!(t(Lang::Pt, Key::ClearHistory), "Limpar");
        assert_eq!(t(Lang::Pt, Key::ClearAll), "Limpar tudo");
        assert_eq!(t(Lang::Pt, Key::ClearExplain), "Os itens fixados são mantidos");
        assert_eq!(t(Lang::Pt, Key::HistoryCleared), "histórico limpo");
        assert_eq!(t(Lang::Pt, Key::RegisterHotkey), "Registrar atalho");
        assert_eq!(t(Lang::Pt, Key::PressKeysHint), "Pressione o novo atalho… (Esc cancela)");
        assert_eq!(t(Lang::Es, Key::ClearAll), "Borrar todo");
        assert_eq!(t(Lang::Es, Key::PressKeysHint), "Pulse el nuevo atajo… (Esc cancela)");
    }

    /// As chaves do v0.2 (popup no cursor, colar direto e OCR) têm linha nos
    /// TSVs: sem elas a UI cairia no inglês sem aviso.
    #[test]
    fn chaves_do_popup_e_do_ocr_saem_do_catalogo() {
        assert_eq!(t(Lang::Pt, Key::TogglePopupMode), "Abrir um popup rápido no cursor");
        assert_eq!(t(Lang::Pt, Key::PopupNoResults), "Nada encontrado");
        assert_eq!(t(Lang::Pt, Key::CopyText), "Copiar texto");
        assert_eq!(t(Lang::Pt, Key::OcrFailed), "Não foi possível ler a imagem");
        assert_eq!(t(Lang::Es, Key::TogglePopupMode), "Abrir una ventana emergente rápida en el cursor");
        assert_eq!(t(Lang::Es, Key::CopyText), "Copiar texto");
        assert_eq!(t(Lang::Es, Key::PopupNoResults), "Nada encontrado");
        // "OCR" não traduz.
        assert_eq!(t(Lang::Pt, Key::OcrButton), "OCR");
    }

    /// As chaves do v0.3 (captura + editor) têm linha nos TSVs: sem elas a aba
    /// cairia no inglês sem aviso. `ToolText` de propósito reaproveita a linha
    /// "Text" do filtro da aba Clipboard (a chave do catálogo é a string).
    #[test]
    fn chaves_da_captura_saem_do_catalogo() {
        assert_eq!(t(Lang::Pt, Key::CaptureHint), "Pressione {hotkey} para capturar");
        assert_eq!(t(Lang::Pt, Key::CaptureFailed), "Não foi possível capturar a tela");
        assert_eq!(t(Lang::Pt, Key::ToolRect), "Retângulo");
        assert_eq!(t(Lang::Pt, Key::ToolArrow), "Seta");
        assert_eq!(t(Lang::Pt, Key::ToolPixelate), "Pixelizar");
        assert_eq!(t(Lang::Pt, Key::ToolBlur), "Desfocar");
        assert_eq!(t(Lang::Pt, Key::Undo), "Desfazer");
        assert_eq!(t(Lang::Pt, Key::ShotSaved), "captura salva");
        assert_eq!(t(Lang::Pt, Key::ToolText), "Texto", "reaproveita a linha do filtro");
        assert_eq!(t(Lang::Es, Key::CaptureHint), "Pulse {hotkey} para capturar");
        assert_eq!(t(Lang::Es, Key::ToolBlur), "Desenfocar");
        assert_eq!(t(Lang::Es, Key::ShotSaveFailed), "No se pudo guardar la captura");
    }

    /// As chaves do v0.4 (aba Ferramentas + Notas) têm linha nos TSVs: sem elas
    /// os painéis novos cairiam no inglês sem aviso.
    #[test]
    fn chaves_das_ferramentas_saem_do_catalogo() {
        assert_eq!(t(Lang::Pt, Key::ToolsClock), "Relógio");
        assert_eq!(t(Lang::Pt, Key::CalcTab), "Calculadora");
        assert_eq!(t(Lang::Pt, Key::UnitsTab), "Unidades");
        assert_eq!(t(Lang::Pt, Key::ClockStopwatch), "Cronômetro");
        assert_eq!(t(Lang::Pt, Key::ClockTimeUp), "o tempo acabou");
        assert_eq!(t(Lang::Pt, Key::ClockPomodoroCycles), "{n} ciclos");
        assert_eq!(t(Lang::Pt, Key::ClockAlarmRinging), "alarme: {label}");
        assert_eq!(t(Lang::Pt, Key::CalcDivZero), "divisão por zero");
        assert_eq!(t(Lang::Pt, Key::CalcUnknownName), "nome desconhecido: {name}");
        assert_eq!(t(Lang::Pt, Key::UnitsTemperature), "Temperatura");
        assert_eq!(t(Lang::Pt, Key::UnitsFrom), "De");
        assert_eq!(t(Lang::Pt, Key::DevUuid), "UUID v4");
        assert_eq!(t(Lang::Pt, Key::DevIsoHint), "Data ISO (ex.: 2026-11-04T14:30:00)");
        assert_eq!(t(Lang::Pt, Key::NotesChars), "{n} caracteres");
        assert_eq!(t(Lang::Pt, Key::NotesSaved), "salvo ✓");
        assert_eq!(t(Lang::Pt, Key::CopyValue), "Copiar");
        assert_eq!(t(Lang::Es, Key::ToolsClock), "Reloj");
        assert_eq!(t(Lang::Es, Key::ClockStopwatch), "Cronómetro");
        assert_eq!(t(Lang::Es, Key::CalcDivZero), "división por cero");
        assert_eq!(t(Lang::Es, Key::UnitsLength), "Longitud");
        assert_eq!(t(Lang::Es, Key::UnitsTo), "A");
        assert_eq!(t(Lang::Es, Key::NotesChars), "{n} caracteres");
        assert_eq!(t(Lang::Es, Key::CopyValue), "Copiar");
        // Marca e símbolos não traduzem.
        assert_eq!(t(Lang::Es, Key::DevUuid), "UUID v4");
        assert_eq!(t(Lang::Pt, Key::DevBase64), "Base64");
    }

    /// As chaves do v0.5 (aba Cor + aba Converter) têm linha nos TSVs: sem elas
    /// as duas abas novas cairiam no inglês sem aviso.
    #[test]
    fn chaves_da_cor_e_do_conversor_saem_do_catalogo() {
        // "Color" reaproveita a linha do editor da captura.
        assert_eq!(t(Lang::Pt, Key::TabPicker), "Cor");
        assert_eq!(t(Lang::Es, Key::TabPicker), "Color");
        assert_eq!(t(Lang::Pt, Key::CaptureColor), "Capturar da tela");
        assert_eq!(t(Lang::Pt, Key::RecentColors), "Cores recentes");
        assert_eq!(t(Lang::Pt, Key::InvalidColor), "cor inválida");
        assert_eq!(t(Lang::Es, Key::InvalidColor), "color inválido");
        assert_eq!(t(Lang::Pt, Key::TabConvert), "Converter");
        assert_eq!(t(Lang::Es, Key::TabConvert), "Convertir");
        assert_eq!(t(Lang::Pt, Key::DropImagesHint), "Arraste imagens aqui");
        assert_eq!(t(Lang::Pt, Key::SelectFiles), "Selecionar arquivos");
        assert_eq!(t(Lang::Pt, Key::Quality), "Qualidade");
        assert_eq!(t(Lang::Pt, Key::ConvertedTo).replace("{name}", "a.png"), "salvo em a.png");
        assert_eq!(t(Lang::Pt, Key::ConvertFailed), "Não foi possível converter");
        assert_eq!(t(Lang::Pt, Key::UnsupportedType), "não é uma imagem suportada");
        assert_eq!(t(Lang::Pt, Key::ConvertWaiting), "aguardando");
        assert_eq!(t(Lang::Pt, Key::ClearDone), "Limpar concluídos");
        assert_eq!(t(Lang::Es, Key::DropImagesHint), "Arrastra imágenes aquí");
        // Símbolos não traduzem.
        assert_eq!(t(Lang::Pt, Key::LabelHex), "Hex");
        assert_eq!(t(Lang::Pt, Key::LabelRgb), "RGB");
    }
}
