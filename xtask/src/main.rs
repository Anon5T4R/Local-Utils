//! Tarefas de build/CI do Local Utils (padrão local-office, versão mínima).
//!
//! - `cargo xtask ci` — o gate local, igual ao CI: fmt, clippy -D warnings, build, test,
//!   cargo-deny e espelhos do local-libs (deny/mirror pulam com aviso se faltar a ferramenta).
//! - `cargo xtask fmt` — `fmt --check` só dos pacotes do workspace (nunca as árvores de `libs/`).
//! - `cargo xtask version set X.Y.Z` (ou `version X.Y.Z`, o jeito antigo) — bump atômico: Cargo.toml do
//!   app + utils-ui + refresh do Cargo.lock.
//! - `cargo xtask check` — confere que as 2 versões estão sincronizadas.
//!
//! Zero dependências: manipulação de texto com std só (política de deps).

use std::fs;
use std::process::Command;

fn main() {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("version") => {
            let mut args = std::env::args().skip(2);
            let first = args.next().unwrap_or_else(|| die("uso: cargo xtask version set X.Y.Z"));
            // `version set X.Y.Z` é o canônico da suíte; `version X.Y.Z` (antigo) continua aceito.
            let v = if first == "set" { args.next().unwrap_or_else(|| die("uso: cargo xtask version set X.Y.Z")) } else { first };
            version(&v);
        }
        Some("check") => check(),
        Some("fmt") => fmt().unwrap_or_else(|e| die(&e)),
        Some("ci") => ci().unwrap_or_else(|e| die(&e)),
        _ => die("uso: cargo xtask <ci|fmt|version set X.Y.Z|check>"),
    }
}

/// (arquivo, prefixo da linha de versão)
fn version_slots() -> Vec<(String, String)> {
    vec![("Cargo.toml".into(), "version = ".into()), ("utils-ui/Cargo.toml".into(), "version = ".into())]
}

fn read_version(path: &str, prefix: &str) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| die(&format!("ler {path}: {e}")));
    let line = text.lines().find(|l| l.trim_start().starts_with(prefix)).unwrap_or_else(|| die(&format!("{path}: linha `{prefix}` não encontrada")));
    line.trim().strip_prefix(prefix).unwrap().trim().trim_matches('"').to_string()
}

fn set_version(path: &str, prefix: &str, v: &str) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| die(&format!("ler {path}: {e}")));
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for line in text.lines() {
        if !done && line.trim_start().starts_with(prefix) {
            let indent = line.len() - line.trim_start().len();
            let quote = "\"";
            out.push_str(&line[..indent]);
            out.push_str(prefix);
            out.push_str(quote);
            out.push_str(v);
            out.push_str(quote);
            done = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    fs::write(path, out).unwrap_or_else(|e| die(&format!("escrever {path}: {e}")));
}

fn version(v: &str) {
    if v.split('.').count() != 3 || !v.chars().all(|c| c.is_ascii_digit() || c == '.') {
        die(&format!("versão inválida: {v} (esperado X.Y.Z)"));
    }
    for (path, prefix) in version_slots() {
        set_version(&path, &prefix, v);
        println!("  {path} -> {v}");
    }
    // Refresh do lock (só o pacote local muda).
    let st = Command::new("cargo").args(["update", "--workspace"]).status().unwrap_or_else(|e| die(&format!("cargo update: {e}")));
    if !st.success() {
        die("cargo update falhou");
    }
    println!("  Cargo.lock atualizado");
    check();
}

fn check() {
    let mut vers = Vec::new();
    for (path, prefix) in version_slots() {
        vers.push((path.clone(), read_version(&path, &prefix)));
    }
    let first = vers[0].1.clone();
    let mut ok = true;
    for (path, v) in &vers {
        if v != &first {
            eprintln!("DESSINCRONIZADO: {path} = {v} (esperado {first})");
            ok = false;
        }
    }
    if ok {
        println!("versões sincronizadas: {first}");
    } else {
        std::process::exit(1);
    }
}

fn die(msg: &str) -> ! {
    eprintln!("xtask: {msg}");
    std::process::exit(1);
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: falhou ao iniciar: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: saiu com {status}"))
    }
}

/// Nomes dos pacotes do workspace (`cargo metadata --no-deps`), sem dependências: o objeto de um pacote é
/// o único que começa por `{"name":"…","version":`; os alvos começam por `"kind"`.
fn workspace_packages() -> Result<Vec<String>, String> {
    let out = cargo().args(["metadata", "--no-deps", "--format-version", "1"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata falhou:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();
    for (i, _) in json.match_indices("{\"name\":\"") {
        let rest = &json[i + 9..];
        if let Some(end) = rest.find('"') {
            if rest[end..].starts_with("\",\"version\":") {
                names.push(rest[..end].to_string());
            }
        }
    }
    if names.is_empty() {
        return Err("cargo metadata: o workspace não lista pacotes".into());
    }
    Ok(names)
}

/// `cargo xtask fmt`: só os pacotes do workspace. `cargo fmt --all` reformataria também as dependências por
/// caminho em `libs/`, que mantêm o estilo de origem.
fn fmt() -> Result<(), String> {
    let members = workspace_packages()?;
    let mut c = cargo();
    c.arg("fmt");
    for name in &members {
        c.args(["-p", name]);
    }
    c.args(["--", "--check"]);
    run(c, "cargo fmt -p <pacotes do workspace> -- --check")
}

/// `cargo xtask ci`: o gate local — o mesmo que `.github/workflows/ci.yml`, na ordem dele, parando na
/// primeira falha (um build local por vez: `MEMORY.md` §4 do dev-notes).
fn ci() -> Result<(), String> {
    fmt()?;
    let mut c = cargo();
    c.args(["clippy", "--locked", "--all-targets", "--", "-D", "warnings"]);
    run(c, "cargo clippy --locked --all-targets -- -D warnings")?;
    let mut c = cargo();
    c.args(["build", "--locked"]);
    run(c, "cargo build --locked")?;
    let mut c = cargo();
    c.args(["test", "--locked"]);
    run(c, "cargo test --locked")?;
    let mut c = cargo();
    c.args(["deny", "--version"]);
    if c.output().is_ok_and(|o| o.status.success()) {
        let mut c = cargo();
        c.args(["deny", "--log-level", "error", "check"]);
        run(c, "cargo deny check")?;
    } else {
        eprintln!("aviso: cargo-deny não instalado (cargo install cargo-deny --locked): pulado, o CI roda");
    }
    let mut c = Command::new("bash");
    c.arg("scripts/check-mirror.sh");
    match c.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("scripts/check-mirror.sh: saiu com {status}")),
        Err(e) => {
            eprintln!("aviso: bash indisponível ({e}): espelhos pulados, o CI roda");
            Ok(())
        }
    }
}
