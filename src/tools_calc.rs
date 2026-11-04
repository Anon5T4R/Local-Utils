//! Calculadora da aba Ferramentas: expressão numérica com parênteses, `+ - * /
//! % ^`, unário `-`/`+`, funções (`sqrt sin cos tan log ln abs`) e as constantes
//! `pi`/`e`.
//!
//! Parser recursive-descent **puro** (nenhum I/O, nenhum estado global): lexer
//! que vira `Vec<Token>`, parser que consome e um `f64` no fim. O erro é TIPADO
//! ([`CalcError`]) de propósito: o texto que o usuário lê sai do catálogo i18n,
//! não de uma mensagem em inglês cravada no parser.
//!
//! Precedência, da menor pra maior: `+ -` · `* / %` · unário `- +` · `^` (associa
//! à direita) · primário (número, função, constante, parênteses).

use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard_ui::copy_new_text;
use crate::history::History;
use crate::ui::{self as kit, PadKey, Skin};

/// Quantas contas ficam no histórico da sessão.
const HISTORY_MAX: usize = 10;
/// Largura da coluna (calculadora é uma coluna estreita e centralizada).
const CONTENT_W: f32 = 440.0;
/// Vão entre as teclas do keypad (a altura de 52px é do [`kit::pad`]).
const KEY_GAP: f32 = 8.0;
/// Altura máxima da lista de histórico (rola além disso).
const HISTORY_H: f32 = 240.0;

/// Peça da expressão (o lexer entrega, o parser consome).
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
}

/// O que pode dar errado ao avaliar. Cada variante tem uma chave do catálogo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcError {
    /// Expressão vazia (só espaços).
    Empty,
    /// Caractere inesperado, operador solto ou sobra de token no fim.
    Syntax,
    /// Faltou abrir/fechar parêntese.
    Unbalanced,
    /// Divisão (ou módulo) por zero.
    DivByZero,
    /// Nome que não é constante nem função.
    UnknownName(String),
    /// Número malformado (ex.: `1.2.3`).
    BadNumber,
    /// Fora do domínio (sqrt de negativo, log de zero/negativo, resultado NaN).
    Domain,
}

impl CalcError {
    /// Chave do catálogo que descreve o erro na língua do usuário.
    fn key(&self) -> Key {
        match self {
            CalcError::Empty | CalcError::Syntax => Key::CalcSyntax,
            CalcError::Unbalanced => Key::CalcUnbalanced,
            CalcError::DivByZero => Key::CalcDivZero,
            CalcError::UnknownName(_) => Key::CalcUnknownName,
            CalcError::BadNumber => Key::CalcBadNumber,
            CalcError::Domain => Key::CalcDomain,
        }
    }

    /// Mensagem pronta (já traduzida; `{name}` preenchido quando for o caso).
    fn message(&self, lang: Lang) -> String {
        let text = i18n::t(lang, self.key());
        match self {
            CalcError::UnknownName(name) => text.replace("{name}", name),
            _ => text.to_string(),
        }
    }
}

/// Quebra a expressão em tokens. `Err` já diz o que houve de errado.
fn lex(src: &str) -> Result<Vec<Token>, CalcError> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                let mut dots = 0;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    if chars[i] == '.' {
                        dots += 1;
                    }
                    i += 1;
                }
                if dots > 1 {
                    return Err(CalcError::BadNumber);
                }
                let raw: String = chars[start..i].iter().collect();
                let value: f64 = raw.parse().map_err(|_| CalcError::BadNumber)?;
                if !value.is_finite() {
                    return Err(CalcError::BadNumber);
                }
                tokens.push(Token::Number(value));
            }
            _ if c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect::<String>().to_lowercase();
                tokens.push(Token::Ident(name));
            }
            _ => return Err(CalcError::Syntax),
        }
    }
    if tokens.is_empty() {
        return Err(CalcError::Empty);
    }
    Ok(tokens)
}

/// Constante nomeada (`pi`, `e`).
fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        _ => None,
    }
}

/// Função de um argumento.
fn apply_fn(name: &str, arg: f64) -> Result<f64, CalcError> {
    match name {
        "sqrt" => {
            if arg < 0.0 {
                Err(CalcError::Domain)
            } else {
                Ok(arg.sqrt())
            }
        }
        "sin" => Ok(arg.sin()),
        "cos" => Ok(arg.cos()),
        "tan" => Ok(arg.tan()),
        "log" => {
            if arg <= 0.0 {
                Err(CalcError::Domain)
            } else {
                Ok(arg.log10())
            }
        }
        "ln" => {
            if arg <= 0.0 {
                Err(CalcError::Domain)
            } else {
                Ok(arg.ln())
            }
        }
        "abs" => Ok(arg.abs()),
        other => Err(CalcError::UnknownName(format!("{other}()"))),
    }
}

/// Consumidor de tokens, um método por nível de precedência.
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// `expr := term (('+' | '-') term)*`
    fn expr(&mut self) -> Result<f64, CalcError> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.pos += 1;
                    value += self.term()?;
                }
                Some(Token::Minus) => {
                    self.pos += 1;
                    value -= self.term()?;
                }
                _ => return Ok(value),
            }
        }
    }

    /// `term := unary (('*' | '/' | '%') unary)*`
    fn term(&mut self) -> Result<f64, CalcError> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.pos += 1;
                    value *= self.unary()?;
                }
                Some(Token::Slash) => {
                    self.pos += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return Err(CalcError::DivByZero);
                    }
                    value /= divisor;
                }
                Some(Token::Percent) => {
                    self.pos += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return Err(CalcError::DivByZero);
                    }
                    value %= divisor;
                }
                _ => return Ok(value),
            }
        }
    }

    /// `unary := ('-' | '+') unary | power` — o sinal vale pro `power` inteiro
    /// (`-2^2` é `-(2^2)`, como em qualquer calculadora).
    fn unary(&mut self) -> Result<f64, CalcError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Plus) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `power := primary ('^' unary)?` — associa à DIREITA (`2^3^2` = 2⁹).
    fn power(&mut self) -> Result<f64, CalcError> {
        let base = self.primary()?;
        if matches!(self.peek(), Some(Token::Caret)) {
            self.pos += 1;
            let exponent = self.unary()?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    /// `primary := número | constante | função '(' expr ')' | '(' expr ')'`
    fn primary(&mut self) -> Result<f64, CalcError> {
        match self.next() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.pos += 1;
                    let arg = self.expr()?;
                    match self.next() {
                        Some(Token::RParen) => apply_fn(&name, arg),
                        _ => Err(CalcError::Unbalanced),
                    }
                } else {
                    constant(&name).ok_or(CalcError::UnknownName(name))
                }
            }
            Some(Token::LParen) => {
                let value = self.expr()?;
                match self.next() {
                    Some(Token::RParen) => Ok(value),
                    _ => Err(CalcError::Unbalanced),
                }
            }
            // Fecha parêntese sobrando, operador solto ou fim da expressão.
            _ => Err(CalcError::Syntax),
        }
    }
}

/// Avalia a expressão. `Err` já é o motivo tipado (a UI traduz).
pub fn eval(src: &str) -> Result<f64, CalcError> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.expr()?;
    // Sobrou token ("1 2", "(1) 3"): é erro de sintaxe, não resultado.
    if parser.pos != parser.tokens.len() {
        return Err(CalcError::Syntax);
    }
    if !value.is_finite() {
        return Err(CalcError::Domain);
    }
    Ok(value)
}

/// Resultado legível: inteiro quando é inteiro, até 10 casas sem zeros à toa e
/// notação científica pro que é pequeno demais pra ler em decimal.
pub fn fmt_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    if value.abs() < 1e-9 {
        return format!("{value:.6e}");
    }
    let text = format!("{value:.10}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Uma conta do histórico da sessão (o que foi digitado e o que deu).
struct Line {
    expr: String,
    value: String,
}

/// Estado da calculadora entre frames.
#[derive(Default)]
pub struct CalcState {
    expr: String,
    /// Último resultado (fica visível grande; clicar copia).
    result: Option<String>,
    /// Motivo do último erro (aparece no visor, em cor de alerta).
    error: Option<String>,
    /// Últimas [`HISTORY_MAX`] contas, da mais nova pra mais antiga.
    history: Vec<Line>,
}

impl CalcState {
    /// Avalia o campo: acerto entra no histórico, erro vira status traduzido.
    fn evaluate(&mut self, lang: Lang) -> Option<String> {
        match eval(&self.expr) {
            Ok(value) => {
                let text = fmt_number(value);
                self.history.insert(0, Line { expr: self.expr.trim().to_string(), value: text.clone() });
                self.history.truncate(HISTORY_MAX);
                self.result = Some(text);
                self.error = None;
                None
            }
            Err(e) => {
                let message = e.message(lang);
                self.result = None;
                self.error = Some(message.clone());
                Some(format!("⚠ {message}"))
            }
        }
    }

    /// Zera o visor (a tecla C e o Esc).
    fn clear(&mut self) {
        self.expr.clear();
        self.result = None;
        self.error = None;
    }
}

/// O que a calculadora devolve pro app (gravar o histórico e mostrar status).
#[derive(Default)]
pub struct Outcome {
    /// Uma conta foi copiada: o app regrava o histórico de clipboard.
    pub history_changed: bool,
    pub status: Option<String>,
}

/// O que uma tecla do keypad faz: só EDITA a string da expressão (o parser e o
/// caminho de colar funções no campo continuam sendo os mesmos).
#[derive(Clone, Copy)]
enum KeyAct {
    Insert(&'static str),
    Backspace,
    Clear,
    Eval,
}

/// O teclado 4×5: dígitos claros, operadores fundos e o `=` no accent. A última
/// fileira tem cinco teclas (C, parênteses, `%` e `=`) e divide a MESMA largura
/// — as teclas acima continuam 4 por linha.
const KEYPAD: [&[(&str, KeyAct, PadKey)]; 5] = [
    &[
        ("7", KeyAct::Insert("7"), PadKey::Digit),
        ("8", KeyAct::Insert("8"), PadKey::Digit),
        ("9", KeyAct::Insert("9"), PadKey::Digit),
        ("÷", KeyAct::Insert("/"), PadKey::Op),
    ],
    &[
        ("4", KeyAct::Insert("4"), PadKey::Digit),
        ("5", KeyAct::Insert("5"), PadKey::Digit),
        ("6", KeyAct::Insert("6"), PadKey::Digit),
        ("×", KeyAct::Insert("*"), PadKey::Op),
    ],
    &[
        ("1", KeyAct::Insert("1"), PadKey::Digit),
        ("2", KeyAct::Insert("2"), PadKey::Digit),
        ("3", KeyAct::Insert("3"), PadKey::Digit),
        ("−", KeyAct::Insert("-"), PadKey::Op),
    ],
    &[
        ("0", KeyAct::Insert("0"), PadKey::Digit),
        (".", KeyAct::Insert("."), PadKey::Digit),
        ("⌫", KeyAct::Backspace, PadKey::Op),
        ("+", KeyAct::Insert("+"), PadKey::Op),
    ],
    &[
        ("C", KeyAct::Clear, PadKey::Op),
        ("(", KeyAct::Insert("("), PadKey::Op),
        (")", KeyAct::Insert(")"), PadKey::Op),
        ("%", KeyAct::Insert("%"), PadKey::Op),
        ("=", KeyAct::Eval, PadKey::Accent),
    ],
];

/// Desenha a calculadora. `texts` é o histórico de clipboard (o "copiar" cria
/// uma entrada nova, como no OCR).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut CalcState, texts: &mut History) -> Outcome {
    let mut out = Outcome::default();

    kit::column(ui, CONTENT_W, |ui| {
        // Esc = C (o teclado físico continua valendo em qualquer foco).
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            state.clear();
        }

        // ── visor: expressão (15px) + resultado herói (30px mono accent) ──
        let width = ui.available_width();
        kit::card(ui, width, |ui| {
            let skin = Skin::of(ui);
            let field_w = ui.available_width();
            let field = ui.add(
                egui::TextEdit::singleline(&mut state.expr)
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::proportional(15.0))
                    .text_color(skin.text)
                    .hint_text(i18n::t(lang, Key::CalcExpression))
                    .margin(egui::Margin::symmetric(0, 4))
                    .desired_width(field_w),
            );
            // Digitou? O erro da conta anterior sai do visor (senão a mensagem
            // fica mentindo sobre a expressão que está na tela).
            if field.changed() {
                state.error = None;
            }
            // ⌫ fora do campo apaga o último caractere (com o campo focado o
            // próprio TextEdit cuida disso).
            if !field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
                state.expr.pop();
                state.error = None;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match (&state.error, &state.result) {
                        // Erro ocupa o lugar do resultado, em corpo menor: a
                        // mensagem é uma frase, não um número.
                        (Some(message), _) => {
                            ui.label(egui::RichText::new(message).color(skin.danger));
                        }
                        (None, Some(value)) => {
                            let label = egui::Label::new(egui::RichText::new(value).font(egui::FontId::monospace(30.0)).strong().color(skin.accent))
                                .truncate()
                                .sense(egui::Sense::click());
                            let resp = ui.add(label).on_hover_text(i18n::t(lang, Key::CopyValue));
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            // O resultado inteiro é copiável (mesmo caminho do
                            // "Copiar" dos outros painéis: vira entrada nova).
                            if resp.clicked() {
                                let (changed, status) = copy_new_text(texts, lang, value);
                                out.history_changed |= changed;
                                out.status = Some(status);
                            }
                        }
                        (None, None) => {
                            ui.label(egui::RichText::new("—").font(egui::FontId::monospace(30.0)).color(skin.muted));
                        }
                    }
                });
            });
        });

        // Enter avalia de qualquer foco (depois de clicar no keypad o campo
        // perde o foco, e a tecla não pode "morrer").
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            out.status = state.evaluate(lang);
        }

        // ── keypad 4×5 ────────────────────────────────────────────────
        ui.add_space(12.0);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(KEY_GAP, KEY_GAP);
            for row in KEYPAD {
                // Cada fileira calcula a própria largura de tecla: a última tem
                // cinco e divide o mesmo espaço.
                let cols = row.len() as f32;
                let key_w = ((width - (cols - 1.0) * KEY_GAP) / cols).floor();
                ui.horizontal(|ui| {
                    for &(label, act, kind) in row {
                        if !kit::pad(ui, label, kind, key_w).clicked() {
                            continue;
                        }
                        match act {
                            KeyAct::Insert(text) => {
                                state.expr.push_str(text);
                                state.error = None;
                            }
                            KeyAct::Backspace => {
                                state.expr.pop();
                                state.error = None;
                            }
                            KeyAct::Clear => state.clear(),
                            KeyAct::Eval => out.status = state.evaluate(lang),
                        }
                    }
                });
            }
        });

        // ── histórico no padrão Clipboard ─────────────────────────────
        ui.add_space(14.0);
        let skin = Skin::of(ui);
        ui.label(egui::RichText::new(i18n::t(lang, Key::CalcHistory)).small().color(skin.muted));
        ui.add_space(4.0);
        if state.history.is_empty() {
            kit::card(ui, width, |ui| {
                let skin = Skin::of(ui);
                ui.label(egui::RichText::new(i18n::t(lang, Key::CalcEmpty)).color(skin.muted));
                ui.label(egui::RichText::new(i18n::t(lang, Key::CalcHint)).small().color(skin.muted));
            });
            return;
        }
        let mut reinsert: Option<usize> = None;
        let mut copy: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("calc-history").max_height(HISTORY_H).auto_shrink([false, true]).show(ui, |ui| {
            for (index, line) in state.history.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
                }
                let inner = rect.shrink2(egui::vec2(8.0, 0.0));
                let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
                row.add(egui::Label::new(egui::RichText::new(format!("{} =", line.expr)).color(skin.muted)).truncate());
                row.add_space(6.0);
                row.label(egui::RichText::new(&line.value).strong().color(skin.text));
                let mut copied = false;
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if resp.hovered() && kit::ghost(ui, "⧉").on_hover_text(i18n::t(lang, Key::CopyValue)).clicked() {
                        copied = true;
                    }
                });
                if copied {
                    copy = Some(line.value.clone());
                } else if resp.clicked() {
                    // Clique na linha re-insere a conta no campo (o uso mais
                    // comum de uma conta antiga é continuar a partir dela).
                    reinsert = Some(index);
                }
            }
        });
        if let Some(index) = reinsert {
            if let Some(line) = state.history.get(index) {
                state.expr = line.expr.clone();
                state.result = Some(line.value.clone());
                state.error = None;
            }
        }
        if let Some(value) = copy {
            let (changed, status) = copy_new_text(texts, lang, &value);
            out.history_changed |= changed;
            out.status = Some(status);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resultado esperado de uma expressão que TEM que avaliar.
    fn ok(src: &str) -> f64 {
        match eval(src) {
            Ok(value) => value,
            Err(e) => panic!("{src:?} deveria avaliar: {e:?}"),
        }
    }

    fn perto(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn precedencia_e_associatividade() {
        assert_eq!(ok("2+3*4"), 14.0);
        assert_eq!(ok("(2+3)*4"), 20.0);
        assert_eq!(ok("10-2-3"), 5.0, "subtração associa à esquerda");
        assert_eq!(ok("100/5/2"), 10.0, "divisão associa à esquerda");
        assert_eq!(ok("2^3^2"), 512.0, "potência associa à direita");
        assert_eq!(ok("-2^2"), -4.0, "o sinal vale pro `power` inteiro");
        assert_eq!(ok("2^-1"), 0.5);
        assert_eq!(ok("-3*-2"), 6.0);
        assert_eq!(ok("10%4"), 2.0);
        assert_eq!(ok("+7"), 7.0);
    }

    #[test]
    fn parenteses_aninhados() {
        assert_eq!(ok("((1+2)*(3+4))"), 21.0);
        assert_eq!(ok("2*(3+(4-1))"), 12.0);
        assert_eq!(ok("(((5)))"), 5.0);
    }

    #[test]
    fn funcoes_e_constantes() {
        assert_eq!(ok("sqrt(9)"), 3.0);
        assert_eq!(ok("abs(-4)"), 4.0);
        assert!(perto(ok("pi"), std::f64::consts::PI));
        assert!(perto(ok("ln(e)"), 1.0));
        assert!(perto(ok("log(100)"), 2.0));
        assert!(perto(ok("sin(0)"), 0.0));
        assert!(perto(ok("cos(0)"), 1.0));
        assert_eq!(ok("2*pi"), std::f64::consts::PI * 2.0);
    }

    #[test]
    fn erro_de_sintaxe() {
        assert_eq!(eval("2+"), Err(CalcError::Syntax));
        assert_eq!(eval("*3"), Err(CalcError::Syntax));
        assert_eq!(eval("1 2"), Err(CalcError::Syntax));
        assert_eq!(eval("2 $ 3"), Err(CalcError::Syntax));
        assert_eq!(eval(""), Err(CalcError::Empty));
        assert_eq!(eval("   "), Err(CalcError::Empty));
        assert_eq!(eval("1.2.3"), Err(CalcError::BadNumber));
    }

    #[test]
    fn parentese_faltando() {
        assert_eq!(eval("(1+2"), Err(CalcError::Unbalanced));
        assert_eq!(eval("sqrt(9"), Err(CalcError::Unbalanced));
        assert_eq!(eval("1+2)"), Err(CalcError::Syntax));
    }

    #[test]
    fn divisao_por_zero() {
        assert_eq!(eval("1/0"), Err(CalcError::DivByZero));
        assert_eq!(eval("1/(2-2)"), Err(CalcError::DivByZero));
        assert_eq!(eval("5%0"), Err(CalcError::DivByZero));
    }

    #[test]
    fn nomes_desconhecidos_e_dominio() {
        assert_eq!(eval("foo"), Err(CalcError::UnknownName("foo".to_string())));
        assert_eq!(eval("foo(1)"), Err(CalcError::UnknownName("foo()".to_string())));
        assert_eq!(eval("sqrt(-1)"), Err(CalcError::Domain));
        assert_eq!(eval("ln(0)"), Err(CalcError::Domain));
    }

    #[test]
    fn formatacao_do_resultado() {
        assert_eq!(fmt_number(14.0), "14");
        assert_eq!(fmt_number(-0.5), "-0.5");
        assert_eq!(fmt_number(0.1 + 0.2), "0.3");
        assert_eq!(fmt_number(1.0 / 3.0), "0.3333333333");
        assert_eq!(fmt_number(0.0), "0");
    }
}
