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
use crate::ui as kit;

/// Quantas contas ficam no histórico da sessão.
const HISTORY_MAX: usize = 10;

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
                None
            }
            Err(e) => {
                self.result = None;
                Some(format!("⚠ {}", e.message(lang)))
            }
        }
    }
}

/// O que a calculadora devolve pro app (gravar o histórico e mostrar status).
#[derive(Default)]
pub struct Outcome {
    /// Uma conta foi copiada: o app regrava o histórico de clipboard.
    pub history_changed: bool,
    pub status: Option<String>,
}

/// Desenha a calculadora. `texts` é o histórico de clipboard (o "copiar" cria
/// uma entrada nova, como no OCR).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut CalcState, texts: &mut History) -> Outcome {
    let mut out = Outcome::default();

    ui.horizontal(|ui| {
        let width = (ui.available_width() - 44.0).max(120.0);
        let resp = ui.add(kit::line(&mut state.expr).hint_text(i18n::t(lang, Key::CalcExpression)).desired_width(width));
        // Enter no campo é o caminho rápido (a pill "=" faz o mesmo).
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || kit::pill(ui, "=", true).clicked() {
            out.status = state.evaluate(lang);
        }
    });
    ui.add_space(8.0);

    if let Some(value) = state.result.clone() {
        let skin = crate::ui::Skin::of(ui);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&value).size(28.0).strong().color(skin.text));
            if kit::pill(ui, i18n::t(lang, Key::CopyValue), false).clicked() {
                let (changed, status) = copy_new_text(texts, lang, &value);
                out.history_changed |= changed;
                out.status = Some(status);
            }
        });
        ui.add_space(10.0);
    }

    ui.label(egui::RichText::new(i18n::t(lang, Key::CalcHistory)).strong().small());
    ui.add_space(4.0);
    if state.history.is_empty() {
        ui.label(egui::RichText::new(i18n::t(lang, Key::CalcEmpty)).weak());
        return out;
    }
    egui::ScrollArea::vertical().id_salt("calc-history").auto_shrink([false, true]).show(ui, |ui| {
        for line in &state.history {
            // A linha inteira é clicável: copiar é o único uso de uma conta
            // antiga (o "quanto deu" vai pro histórico do clipboard).
            let label = format!("{} = {}", line.expr, line.value);
            let resp = ui.add(egui::Label::new(egui::RichText::new(label)).sense(egui::Sense::click()).truncate());
            if resp.clicked() {
                let (changed, status) = copy_new_text(texts, lang, &line.value);
                out.history_changed |= changed;
                out.status = Some(status);
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
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
