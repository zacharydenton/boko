//! [`Math`] tree → LaTeX.
//!
//! Presentation math maps structurally onto LaTeX (`msubsup` → `{}_{}^{}`,
//! `mfrac` → `\frac{}{}`, a fenced `mtable` → a `pmatrix`, …). Token text is
//! literal Unicode; a small map turns the operators and relations that don't
//! render as bare Unicode into commands (∑ → `\sum`), and known function
//! identifiers into their control words (`sin` → `\sin`). Everything else
//! passes through — GitHub's MathJax renders Unicode Greek and most symbols
//! directly.
//!
//! `from_latex` (markdown import) is not yet implemented.

use super::{Math, MathExpr, TokenKind};

/// Render a [`Math`] as a delimited LaTeX string: `$$…$$` for display math,
/// `$…$` for inline. Ready to drop into GitHub-flavored Markdown.
pub fn to_latex(math: &Math) -> String {
    let body = to_latex_body(&math.expr);
    if math.display {
        format!("$$\n{}\n$$", body)
    } else {
        format!("${}$", body)
    }
}

/// Render the undelimited LaTeX for an expression.
pub fn to_latex_body(expr: &MathExpr) -> String {
    let mut out = String::new();
    write_latex(expr, &mut out);
    out.trim().to_string()
}

fn write_latex(expr: &MathExpr, out: &mut String) {
    match expr {
        MathExpr::Row(items) => {
            for it in items {
                write_latex(it, out);
            }
        }
        MathExpr::Token { kind, text } => write_token(*kind, text, out),
        MathExpr::Sub(b, s) => {
            group(out, b);
            out.push('_');
            script(out, s);
        }
        MathExpr::Sup(b, s) => {
            group(out, b);
            out.push('^');
            script(out, s);
        }
        MathExpr::SubSup(b, sub, sup) => {
            group(out, b);
            out.push('_');
            script(out, sub);
            out.push('^');
            script(out, sup);
        }
        MathExpr::Under { base, under } => under_over(out, base, Some(under), None),
        MathExpr::Over { base, over } => under_over(out, base, None, Some(over)),
        MathExpr::UnderOver { base, under, over } => under_over(out, base, Some(under), Some(over)),
        MathExpr::Frac(n, d) => {
            out.push_str("\\frac");
            braces(out, n);
            braces(out, d);
        }
        MathExpr::Sqrt(x) => {
            out.push_str("\\sqrt");
            braces(out, x);
        }
        MathExpr::Root(i, x) => {
            out.push_str("\\sqrt[");
            write_latex(i, out);
            out.push(']');
            braces(out, x);
        }
        MathExpr::Fenced { open, close, body } => write_fenced(out, open, close, body),
        MathExpr::Table { rows, .. } => write_matrix(out, "matrix", rows),
        MathExpr::Space => out.push_str("\\; "),
        MathExpr::Raw { latex, .. } => {
            if let Some(l) = latex {
                out.push_str(l);
            }
        }
    }
}

/// A big operator that takes its scripts as limits (`\sum_{}^{}`).
fn big_operator(expr: &MathExpr) -> Option<&'static str> {
    let MathExpr::Token {
        kind: TokenKind::Op,
        text,
    } = expr
    else {
        return None;
    };
    Some(match text.trim() {
        "∑" => "\\sum",
        "∏" => "\\prod",
        "∐" => "\\coprod",
        "∫" => "\\int",
        "∬" => "\\iint",
        "∭" => "\\iiint",
        "∮" => "\\oint",
        "⋃" => "\\bigcup",
        "⋂" => "\\bigcap",
        "⨁" => "\\bigoplus",
        "⨂" => "\\bigotimes",
        "lim" => "\\lim",
        _ => return None,
    })
}

/// An accent glyph placed over a base (`\hat{x}`, `\bar{x}`).
fn accent_command(over: &MathExpr) -> Option<&'static str> {
    let MathExpr::Token { text, .. } = over else {
        return None;
    };
    Some(match text.trim() {
        "^" | "ˆ" => "\\hat",
        "~" | "˜" => "\\tilde",
        "¯" | "‾" | "―" => "\\bar",
        "→" | "⃗" => "\\vec",
        "˙" => "\\dot",
        "¨" => "\\ddot",
        "ˇ" => "\\check",
        "˘" => "\\breve",
        "˚" => "\\mathring",
        _ => return None,
    })
}

fn under_over(
    out: &mut String,
    base: &MathExpr,
    under: Option<&MathExpr>,
    over: Option<&MathExpr>,
) {
    // Big operators (∑, ∫, lim) take limits as sub/superscripts.
    if let Some(cmd) = big_operator(base) {
        out.push_str(cmd);
        if let Some(u) = under {
            out.push('_');
            script(out, u);
        }
        if let Some(o) = over {
            out.push('^');
            script(out, o);
        }
        return;
    }
    // A bare over-accent (bar, hat, vec) with no under-script.
    if under.is_none()
        && let Some(o) = over
        && let Some(cmd) = accent_command(o)
    {
        out.push_str(cmd);
        braces(out, base);
        return;
    }
    // General case: \overset over the base, then \underset around that.
    // `\overset{over}{base}` and `\underset{under}{inner}` compose to
    // `\underset{under}{\overset{over}{base}}`.
    let mut inner = to_latex_body_raw(base);
    if let Some(o) = over {
        let mut wrapped = String::from("\\overset");
        braces(&mut wrapped, o);
        braces_str(&mut wrapped, &inner);
        inner = wrapped;
    }
    if let Some(u) = under {
        out.push_str("\\underset");
        braces(out, u);
        braces_str(out, &inner);
    } else {
        out.push_str(&inner);
    }
}

fn write_fenced(out: &mut String, open: &str, close: &str, body: &MathExpr) {
    // A fenced matrix becomes a delimited matrix environment.
    if let MathExpr::Table { rows, .. } = body {
        let env = match open.trim() {
            "(" => "pmatrix",
            "[" => "bmatrix",
            "{" => "Bmatrix",
            "|" => "vmatrix",
            "‖" | "∥" => "Vmatrix",
            _ => {
                // Unknown fence: draw delimiters explicitly around a plain matrix.
                out.push_str("\\left");
                push_delim(out, open, true);
                write_matrix(out, "matrix", rows);
                out.push_str("\\right");
                push_delim(out, close, false);
                return;
            }
        };
        write_matrix(out, env, rows);
        return;
    }
    out.push_str("\\left");
    push_delim(out, open, true);
    write_latex(body, out);
    out.push_str("\\right");
    push_delim(out, close, false);
}

fn write_matrix(out: &mut String, env: &str, rows: &[Vec<MathExpr>]) {
    out.push_str("\\begin{");
    out.push_str(env);
    out.push('}');
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            out.push_str(" \\\\ ");
        }
        for (c, cell) in row.iter().enumerate() {
            if c > 0 {
                out.push_str(" & ");
            }
            write_latex(cell, out);
        }
    }
    out.push_str("\\end{");
    out.push_str(env);
    out.push('}');
}

/// Push a fence delimiter as its LaTeX form (`\{` for a brace, `.` for none).
fn push_delim(out: &mut String, glyph: &str, _open: bool) {
    let g = glyph.trim();
    match g {
        "" => out.push('.'),
        "{" => out.push_str("\\{"),
        "}" => out.push_str("\\}"),
        "|" => out.push('|'),
        "‖" | "∥" => out.push_str("\\|"),
        "⌊" => out.push_str("\\lfloor "),
        "⌋" => out.push_str("\\rfloor "),
        "⌈" => out.push_str("\\lceil "),
        "⌉" => out.push_str("\\rceil "),
        "⟨" => out.push_str("\\langle "),
        "⟩" => out.push_str("\\rangle "),
        _ => out.push_str(g),
    }
}

/// Render a sub/superscript after `^`/`_`: a single atom stays bare (`x^2`),
/// otherwise it is braced (`x^{ab}`).
fn script(out: &mut String, expr: &MathExpr) {
    let s = to_latex_body_raw(expr);
    if is_single_atom(&s) {
        out.push_str(&s);
    } else {
        out.push('{');
        out.push_str(&s);
        out.push('}');
    }
}

/// Render `expr` as the base before `^`/`_`: single char stays bare, else
/// braced (so `{a+b}^2` groups correctly).
fn group(out: &mut String, expr: &MathExpr) {
    script(out, expr);
}

/// Render `expr` as a control-word argument — always braced. After a command
/// like `\sqrt`/`\vec`/`\frac`, a bare single character would fuse into the
/// command name (`\sqrtx`), so arguments always brace.
fn braces(out: &mut String, expr: &MathExpr) {
    braces_str(out, &to_latex_body_raw(expr));
}

fn braces_str(out: &mut String, s: &str) {
    out.push('{');
    out.push_str(s);
    out.push('}');
}

fn to_latex_body_raw(expr: &MathExpr) -> String {
    let mut s = String::new();
    write_latex(expr, &mut s);
    s
}

/// Whether a rendered fragment is a single "atom" that needs no braces: one
/// character, or a single control word like `\alpha`.
fn is_single_atom(s: &str) -> bool {
    let t = s.trim();
    if t.chars().count() == 1 {
        return true;
    }
    // A lone control word: backslash + letters, nothing else.
    if let Some(rest) = t.strip_prefix('\\') {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphabetic());
    }
    false
}

/// Known function-name identifiers that map to LaTeX control words.
fn function_command(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "\\sin",
        "cos" => "\\cos",
        "tan" => "\\tan",
        "cot" => "\\cot",
        "sec" => "\\sec",
        "csc" => "\\csc",
        "sinh" => "\\sinh",
        "cosh" => "\\cosh",
        "tanh" => "\\tanh",
        "arcsin" => "\\arcsin",
        "arccos" => "\\arccos",
        "arctan" => "\\arctan",
        "log" => "\\log",
        "ln" => "\\ln",
        "exp" => "\\exp",
        "lim" => "\\lim",
        "max" => "\\max",
        "min" => "\\min",
        "sup" => "\\sup",
        "inf" => "\\inf",
        "det" => "\\det",
        "gcd" => "\\gcd",
        "deg" => "\\deg",
        "dim" => "\\dim",
        "ker" => "\\ker",
        "arg" => "\\arg",
        "mod" => "\\bmod",
        _ => return None,
    })
}

fn write_token(kind: TokenKind, text: &str, out: &mut String) {
    let t = text.trim();
    match kind {
        TokenKind::Num => push_escaped_math_mode(out, t),
        TokenKind::Text => {
            out.push_str("\\text{");
            push_escaped_text_mode(out, text); // preserve internal spaces
            out.push('}');
        }
        TokenKind::Ident => {
            if let Some(cmd) = function_command(t) {
                out.push_str(cmd);
                out.push(' ');
            } else if t.chars().count() > 1 && t.chars().all(|c| c.is_ascii_alphabetic()) {
                // Multi-letter identifier that isn't a known function: upright.
                out.push_str("\\mathrm{");
                out.push_str(t);
                out.push('}');
            } else {
                push_symbol(out, t);
            }
        }
        TokenKind::Op => {
            // Invisible operators produce nothing.
            match t {
                "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}" => {}
                _ => push_symbol(out, t),
            }
        }
    }
}

/// Emit a token, mapping known operator/relation Unicode to LaTeX commands;
/// pass everything else through (MathJax renders Unicode Greek and most
/// symbols directly), escaping LaTeX-special ASCII on the way.
fn push_symbol(out: &mut String, t: &str) {
    if let Some(cmd) = symbol_command(t) {
        out.push_str(cmd);
        out.push(' ');
    } else {
        push_escaped_math_mode(out, t);
    }
}

/// Escape LaTeX-special characters for text mode (inside `\text{…}`). A raw
/// `%` starts a comment (KaTeX silently eats the rest of the equation) and a
/// raw `$` terminates math mode, so unescaped specials corrupt the output.
fn push_escaped_text_mode(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
}

/// Escape LaTeX-special ASCII in math mode (bare number/operator tokens,
/// e.g. `<mn>50%</mn>` or `<mo>&</mo>`). Non-special characters — including
/// all Unicode math symbols — pass through untouched.
fn push_escaped_math_mode(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\backslash "),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '~' => out.push_str("\\text{\\textasciitilde}"),
            '^' => out.push_str("\\text{\\textasciicircum}"),
            _ => out.push(c),
        }
    }
}

/// Unicode → LaTeX command for operators/relations that don't render well as
/// bare Unicode. Greek and ordinary letters intentionally pass through.
fn symbol_command(t: &str) -> Option<&'static str> {
    Some(match t {
        "×" => "\\times",
        "÷" => "\\div",
        "⋅" | "·" => "\\cdot",
        "∗" => "\\ast",
        "±" => "\\pm",
        "∓" => "\\mp",
        "≤" => "\\leq",
        "≥" => "\\geq",
        "≠" => "\\neq",
        "≈" => "\\approx",
        "≡" => "\\equiv",
        "≅" => "\\cong",
        "∼" => "\\sim",
        "∝" => "\\propto",
        "→" => "\\to",
        "←" => "\\leftarrow",
        "↔" => "\\leftrightarrow",
        "⇒" => "\\Rightarrow",
        "⇐" => "\\Leftarrow",
        "⇔" => "\\Leftrightarrow",
        "↦" => "\\mapsto",
        "⟶" => "\\longrightarrow",
        "∞" => "\\infty",
        "∂" => "\\partial",
        "∇" => "\\nabla",
        "∈" => "\\in",
        "∉" => "\\notin",
        "∋" => "\\ni",
        "⊂" => "\\subset",
        "⊆" => "\\subseteq",
        "⊃" => "\\supset",
        "⊇" => "\\supseteq",
        "∪" => "\\cup",
        "∩" => "\\cap",
        "∅" => "\\emptyset",
        "∀" => "\\forall",
        "∃" => "\\exists",
        "¬" => "\\neg",
        "∧" => "\\wedge",
        "∨" => "\\vee",
        "⊕" => "\\oplus",
        "⊗" => "\\otimes",
        "…" => "\\ldots",
        "⋯" => "\\cdots",
        "⋮" => "\\vdots",
        "⋱" => "\\ddots",
        "√" => "\\surd",
        "∠" => "\\angle",
        "°" => "^\\circ",
        "′" => "'",
        "″" => "''",
        "ℝ" => "\\mathbb{R}",
        "ℕ" => "\\mathbb{N}",
        "ℤ" => "\\mathbb{Z}",
        "ℚ" => "\\mathbb{Q}",
        "ℂ" => "\\mathbb{C}",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// LaTeX → tree (the reverse spoke)
// ---------------------------------------------------------------------------

/// Parse a LaTeX math string (body only, no `$` delimiters) into a tree.
/// Covers the closure of what [`to_latex_body`] emits — commands outside the
/// subset return `None` so callers can fall back to [`MathExpr::Raw`].
pub fn from_latex(src: &str) -> Option<MathExpr> {
    let mut p = LatexParser {
        chars: src.chars().collect(),
        pos: 0,
    };
    let items = p.parse_sequence(&[])?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return None; // trailing unconsumed input
    }
    Some(row_or_single(items))
}

/// Parse a delimited LaTeX math snippet (`$…$` inline or `$$…$$` display)
/// into a [`Math`].
pub fn parse_latex_math(src: &str) -> Option<crate::math::Math> {
    let t = src.trim();
    let (body, display) = if let Some(b) = t.strip_prefix("$$").and_then(|x| x.strip_suffix("$$")) {
        (b, true)
    } else if let Some(b) = t.strip_prefix('$').and_then(|x| x.strip_suffix('$')) {
        (b, false)
    } else {
        (t, false)
    };
    Some(crate::math::Math {
        expr: from_latex(body)?,
        display,
        alttext: None,
    })
}

fn row_or_single(mut items: Vec<MathExpr>) -> MathExpr {
    if items.len() == 1 {
        items.pop().unwrap()
    } else {
        MathExpr::Row(items)
    }
}

/// LaTeX command → literal Unicode token text (reverse of `symbol_command`
/// and `big_operator`).
fn command_symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "Rightarrow" => "⇒",
        "angle" => "∠",
        "approx" => "≈",
        "ast" => "∗",
        "bigcap" => "⋂",
        "bigcup" => "⋃",
        "bigoplus" => "⨁",
        "bigotimes" => "⨂",
        "cap" => "∩",
        "cdot" => "⋅",
        "cdots" => "⋯",
        "cong" => "≅",
        "coprod" => "∐",
        "cup" => "∪",
        "ddots" => "⋱",
        "div" => "÷",
        "emptyset" => "∅",
        "equiv" => "≡",
        "exists" => "∃",
        "forall" => "∀",
        "geq" => "≥",
        "iiint" => "∭",
        "iint" => "∬",
        "in" => "∈",
        "infty" => "∞",
        "int" => "∫",
        "ldots" => "…",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "leq" => "≤",
        "longrightarrow" => "⟶",
        "mapsto" => "↦",
        "mp" => "∓",
        "nabla" => "∇",
        "neg" => "¬",
        "neq" => "≠",
        "ni" => "∋",
        "notin" => "∉",
        "oint" => "∮",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "partial" => "∂",
        "pm" => "±",
        "prod" => "∏",
        "propto" => "∝",
        "sim" => "∼",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "sum" => "∑",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "surd" => "√",
        "times" => "×",
        "to" => "→",
        "vdots" => "⋮",
        "vee" => "∨",
        "wedge" => "∧",
        // Greek letters pass through to_latex as Unicode, but accept the
        // commands for hand-written LaTeX.
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "dots" => "…",
        _ => return None,
    })
}

/// Accent command → over-script character (reverse of `accent_command`).
fn command_accent(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "hat" => "^",
        "tilde" => "˜",
        "bar" | "overline" => "¯",
        "vec" | "overrightarrow" => "→",
        "dot" => "˙",
        "ddot" => "¨",
        "check" => "ˇ",
        "breve" => "˘",
        "mathring" => "˚",
        _ => return None,
    })
}

struct LatexParser {
    chars: Vec<char>,
    pos: usize,
}

impl LatexParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn at_stop(&self, stops: &[&str]) -> bool {
        match self.peek() {
            None => true,
            Some('}') => stops.contains(&"}"),
            Some(']') => stops.contains(&"]"),
            Some('&') => stops.contains(&"&"),
            Some('\\') => {
                let cmd = self.peek_command();
                (stops.contains(&"\\\\") && cmd.as_deref() == Some("\\"))
                    || (stops.contains(&"right") && cmd.as_deref() == Some("right"))
                    || (stops.contains(&"end") && cmd.as_deref() == Some("end"))
            }
            _ => false,
        }
    }

    fn peek_command(&self) -> Option<String> {
        if self.peek() != Some('\\') {
            return None;
        }
        let mut i = self.pos + 1;
        let first = self.chars.get(i)?;
        if !first.is_ascii_alphabetic() {
            return Some(first.to_string());
        }
        let mut out = String::new();
        while let Some(&c) = self.chars.get(i) {
            if c.is_ascii_alphabetic() {
                out.push(c);
                i += 1;
            } else {
                break;
            }
        }
        Some(out)
    }

    fn read_command(&mut self) -> Option<String> {
        let cmd = self.peek_command()?;
        self.pos += 1 + cmd.chars().count(); // backslash + name
        Some(cmd)
    }

    /// Items until a stop marker (which is left unconsumed).
    fn parse_sequence(&mut self, stops: &[&str]) -> Option<Vec<MathExpr>> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.at_stop(stops) {
                return Some(items);
            }
            items.push(self.parse_scripted()?);
        }
    }

    /// `{…}` group, or a single atom when no brace follows.
    fn parse_group(&mut self) -> Option<MathExpr> {
        self.skip_ws();
        if self.peek() == Some('{') {
            self.bump();
            let items = self.parse_sequence(&["}"])?;
            if self.bump() != Some('}') {
                return None;
            }
            Some(row_or_single(items))
        } else {
            self.parse_atom()
        }
    }

    /// An atom plus any `^`/`_` scripts.
    fn parse_scripted(&mut self) -> Option<MathExpr> {
        let base = self.parse_atom()?;
        let mut sub: Option<MathExpr> = None;
        let mut sup: Option<MathExpr> = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('_') if sub.is_none() => {
                    self.bump();
                    sub = Some(self.parse_group()?);
                }
                Some('^') if sup.is_none() => {
                    self.bump();
                    sup = Some(self.parse_group()?);
                }
                _ => break,
            }
        }
        let is_big = matches!(
            &base,
            MathExpr::Token { kind: TokenKind::Op, text }
                if matches!(text.as_str(), "∑" | "∏" | "∐" | "⋃" | "⋂" | "⋀" | "⋁"))
            || matches!(&base, MathExpr::Token { kind: TokenKind::Ident, text } if text == "lim");
        Some(match (sub, sup) {
            (None, None) => base,
            (Some(s), None) if is_big => MathExpr::Under {
                base: Box::new(base),
                under: Box::new(s),
            },
            (None, Some(s)) if is_big => MathExpr::Over {
                base: Box::new(base),
                over: Box::new(s),
            },
            (Some(lo), Some(hi)) if is_big => MathExpr::UnderOver {
                base: Box::new(base),
                under: Box::new(lo),
                over: Box::new(hi),
            },
            (Some(s), None) => MathExpr::Sub(Box::new(base), Box::new(s)),
            (None, Some(s)) => MathExpr::Sup(Box::new(base), Box::new(s)),
            (Some(lo), Some(hi)) => MathExpr::SubSup(Box::new(base), Box::new(lo), Box::new(hi)),
        })
    }

    fn parse_atom(&mut self) -> Option<MathExpr> {
        self.skip_ws();
        let c = self.peek()?;
        if c == '\\' {
            return self.parse_command();
        }
        if c == '{' {
            return self.parse_group();
        }
        self.bump();
        Some(match c {
            'a'..='z' | 'A'..='Z' => MathExpr::Token {
                kind: TokenKind::Ident,
                text: c.to_string(),
            },
            '0'..='9' => {
                let mut num = c.to_string();
                while matches!(self.peek(), Some(d) if d.is_ascii_digit() || d == '.') {
                    num.push(self.bump().unwrap());
                }
                MathExpr::Token {
                    kind: TokenKind::Num,
                    text: num,
                }
            }
            _ => MathExpr::Token {
                kind: TokenKind::Op,
                text: c.to_string(),
            },
        })
    }

    fn parse_command(&mut self) -> Option<MathExpr> {
        let cmd = self.read_command()?;
        match cmd.as_str() {
            "frac" => Some(MathExpr::Frac(
                Box::new(self.parse_group()?),
                Box::new(self.parse_group()?),
            )),
            "sqrt" => {
                self.skip_ws();
                if self.peek() == Some('[') {
                    self.bump();
                    let idx = self.parse_sequence(&["]"])?;
                    if self.bump() != Some(']') {
                        return None;
                    }
                    Some(MathExpr::Root(
                        Box::new(row_or_single(idx)),
                        Box::new(self.parse_group()?),
                    ))
                } else {
                    Some(MathExpr::Sqrt(Box::new(self.parse_group()?)))
                }
            }
            "text" | "mathrm" => {
                self.skip_ws();
                if self.bump() != Some('{') {
                    return None;
                }
                let mut out = String::new();
                let mut depth = 1;
                while let Some(ch) = self.bump() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    if depth > 0 {
                        out.push(ch);
                    }
                }
                Some(MathExpr::Token {
                    kind: if cmd == "text" {
                        TokenKind::Text
                    } else {
                        TokenKind::Ident
                    },
                    text: unescape_text_mode(&out),
                })
            }
            "left" => {
                let open = self.read_delimiter()?;
                let body = self.parse_sequence(&["right"])?;
                let r = self.read_command()?;
                if r != "right" {
                    return None;
                }
                let close = self.read_delimiter()?;
                Some(MathExpr::Fenced {
                    open,
                    close,
                    body: Box::new(row_or_single(body)),
                })
            }
            "begin" => self.parse_environment(),
            "bmod" => Some(MathExpr::Token {
                kind: TokenKind::Ident,
                text: "mod".to_string(),
            }),
            "," | ";" | "!" | "quad" | "qquad" => Some(MathExpr::Space),
            "backslash" => Some(MathExpr::Token {
                kind: TokenKind::Op,
                text: "\\".to_string(),
            }),
            _ => {
                if let Some(sym) = command_symbol(&cmd) {
                    return Some(MathExpr::Token {
                        kind: TokenKind::Op,
                        text: sym.to_string(),
                    });
                }
                if let Some(acc) = command_accent(&cmd) {
                    let base = self.parse_group()?;
                    return Some(MathExpr::Over {
                        base: Box::new(base),
                        over: Box::new(MathExpr::Token {
                            kind: TokenKind::Op,
                            text: acc.to_string(),
                        }),
                    });
                }
                if function_command(&cmd).is_some() {
                    return Some(MathExpr::Token {
                        kind: TokenKind::Ident,
                        text: cmd,
                    });
                }
                // Escaped specials: \$ \% \& \# \_ \{ \}
                if cmd.chars().count() == 1
                    && matches!(
                        cmd.chars().next(),
                        Some('$' | '%' | '&' | '#' | '_' | '{' | '}')
                    )
                {
                    return Some(MathExpr::Token {
                        kind: TokenKind::Op,
                        text: cmd,
                    });
                }
                None
            }
        }
    }

    fn read_delimiter(&mut self) -> Option<String> {
        self.skip_ws();
        match self.peek()? {
            '\\' => {
                let cmd = self.read_command()?;
                match cmd.as_str() {
                    "{" | "}" => Some(cmd),
                    "langle" => Some("⟨".into()),
                    "rangle" => Some("⟩".into()),
                    "|" => Some("‖".into()),
                    _ => None,
                }
            }
            '.' => {
                self.bump();
                Some(String::new())
            }
            c @ ('(' | ')' | '[' | ']' | '|') => {
                self.bump();
                Some(c.to_string())
            }
            _ => None,
        }
    }

    fn parse_environment(&mut self) -> Option<MathExpr> {
        self.skip_ws();
        if self.bump() != Some('{') {
            return None;
        }
        let mut env = String::new();
        while let Some(c) = self.bump() {
            if c == '}' {
                break;
            }
            env.push(c);
        }
        let (open, close) = match env.as_str() {
            "matrix" => ("", ""),
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "Bmatrix" => ("{", "}"),
            "vmatrix" => ("|", "|"),
            "cases" => ("{", ""),
            _ => return None,
        };
        let mut rows: Vec<Vec<MathExpr>> = Vec::new();
        let mut row: Vec<MathExpr> = Vec::new();
        loop {
            let cell = self.parse_sequence(&["&", "\\\\", "end"])?;
            row.push(row_or_single(cell));
            self.skip_ws();
            match self.peek() {
                Some('&') => {
                    self.bump();
                }
                Some('\\') => {
                    let cmd = self.read_command()?;
                    match cmd.as_str() {
                        "\\" => {
                            rows.push(std::mem::take(&mut row));
                        }
                        "end" => {
                            rows.push(std::mem::take(&mut row));
                            self.skip_ws();
                            if self.bump() != Some('{') {
                                return None;
                            }
                            while let Some(c) = self.bump() {
                                if c == '}' {
                                    break;
                                }
                            }
                            let table = MathExpr::Table {
                                rows,
                                aligns: vec![],
                            };
                            return Some(if open.is_empty() && close.is_empty() {
                                table
                            } else {
                                MathExpr::Fenced {
                                    open: open.to_string(),
                                    close: close.to_string(),
                                    body: Box::new(table),
                                }
                            });
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
    }
}

/// Undo text-mode escaping (`\%` → `%`, `\textbackslash{}` → `\`).
fn unescape_text_mode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut word = String::new();
        while matches!(chars.peek(), Some(ch) if ch.is_ascii_alphabetic()) {
            word.push(chars.next().unwrap());
        }
        match word.as_str() {
            "" => {
                if let Some(ch) = chars.next() {
                    out.push(ch);
                }
            }
            "textbackslash" => {
                out.push('\\');
                if chars.peek() == Some(&'{') {
                    chars.next();
                    chars.next(); // {}
                }
            }
            "textasciitilde" => {
                out.push('~');
                if chars.peek() == Some(&'{') {
                    chars.next();
                    chars.next();
                }
            }
            "textasciicircum" => {
                out.push('^');
                if chars.peek() == Some(&'{') {
                    chars.next();
                    chars.next();
                }
            }
            w => {
                out.push('\\');
                out.push_str(w);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> MathExpr {
        MathExpr::Token {
            kind: TokenKind::Ident,
            text: s.into(),
        }
    }
    fn num(s: &str) -> MathExpr {
        MathExpr::Token {
            kind: TokenKind::Num,
            text: s.into(),
        }
    }
    fn op(s: &str) -> MathExpr {
        MathExpr::Token {
            kind: TokenKind::Op,
            text: s.into(),
        }
    }
    fn latex(e: MathExpr) -> String {
        to_latex_body(&e)
    }

    #[test]
    fn latex_round_trip_closure() {
        // from_latex must be closed over to_latex's output: re-emitting a
        // parsed string reproduces it exactly.
        for src in [
            "x_{1}",
            "E=mc^{2}",
            "\\frac{a+b}{2c}",
            "\\sqrt{x}",
            "\\sqrt[3]{x}",
            "\\vec{x}",
            "x_{i}^{2}",
            "\\sum _{i=1}^{n}x_{i}",
            "\\left(a+b\\right)",
            "\\begin{pmatrix}a & b\\\\c & d\\end{pmatrix}",
            "\\text{if }x>0",
            "α+β",
        ] {
            let parsed = from_latex(src).unwrap_or_else(|| panic!("parse failed: {src}"));
            let emitted = to_latex_body(&parsed);
            let reparsed = from_latex(&emitted)
                .unwrap_or_else(|| panic!("re-parse failed: {emitted} (from {src})"));
            assert_eq!(
                to_latex_body(&reparsed),
                emitted,
                "round trip not stable for {src}"
            );
        }
    }

    #[test]
    fn from_latex_structures() {
        assert!(matches!(
            from_latex("\\frac{a}{b}"),
            Some(MathExpr::Frac(..))
        ));
        assert!(matches!(
            from_latex("\\sum_{i=1}^{n}"),
            Some(MathExpr::UnderOver { .. })
        ));
        assert!(matches!(from_latex("x_1"), Some(MathExpr::Sub(..))));
        assert!(matches!(
            from_latex("\\begin{pmatrix}a\\\\b\\end{pmatrix}"),
            Some(MathExpr::Fenced { .. })
        ));
        // Unknown commands decline rather than guessing.
        assert!(from_latex("\\notacommand{x}").is_none());
    }

    #[test]
    fn latex_specials_are_escaped() {
        // `%` starts a KaTeX comment (silently eats the rest of the
        // equation), `$` terminates math mode, `&` is a tabular separator —
        // raw occurrences in source tokens must be escaped.
        assert_eq!(latex(num("50%")), "50\\%");
        assert_eq!(latex(op("&")), "\\&");
        assert_eq!(
            latex(MathExpr::Token {
                kind: TokenKind::Text,
                text: "costs $5 & 10%_off".into(),
            }),
            "\\text{costs \\$5 \\& 10\\%\\_off}"
        );
    }

    #[test]
    fn single_char_command_argument_is_braced() {
        // A bare single-char arg would fuse into the control word (`\sqrtx`,
        // `\vecx`) and become an undefined command — arguments must brace.
        assert_eq!(latex(MathExpr::Sqrt(Box::new(id("x")))), "\\sqrt{x}");
        assert_eq!(
            latex(MathExpr::Over {
                base: Box::new(id("x")),
                over: Box::new(op("→")),
            }),
            "\\vec{x}"
        );
        assert_eq!(
            latex(MathExpr::Frac(Box::new(num("1")), Box::new(id("n")))),
            "\\frac{1}{n}"
        );
    }

    #[test]
    fn single_atom_script_is_not_braced() {
        // After `^`/`_` a single atom needs no braces: `x^2`, not `x^{2}`.
        assert_eq!(
            latex(MathExpr::Sup(Box::new(id("x")), Box::new(num("2")))),
            "x^2"
        );
        assert_eq!(
            latex(MathExpr::Sub(Box::new(id("x")), Box::new(id("i")))),
            "x_i"
        );
        // A compound script does brace.
        assert_eq!(
            latex(MathExpr::Sup(
                Box::new(id("x")),
                Box::new(MathExpr::Row(vec![id("a"), op("+"), id("b")]))
            )),
            "x^{a+b}"
        );
    }

    #[test]
    fn subsup_and_bigop_limits() {
        assert_eq!(
            latex(MathExpr::SubSup(
                Box::new(id("x")),
                Box::new(num("1")),
                Box::new(num("2"))
            )),
            "x_1^2"
        );
        // ∑ takes its scripts as limits: \sum_{i=1}^n.
        let sum = MathExpr::UnderOver {
            base: Box::new(op("∑")),
            under: Box::new(MathExpr::Row(vec![id("i"), op("="), num("1")])),
            over: Box::new(id("n")),
        };
        assert_eq!(latex(sum), "\\sum_{i=1}^n");
    }

    #[test]
    fn fenced_matrix_becomes_pmatrix() {
        let table = MathExpr::Table {
            rows: vec![vec![num("1"), num("2")], vec![num("3"), num("4")]],
            aligns: vec![],
        };
        let fenced = MathExpr::Fenced {
            open: "(".into(),
            close: ")".into(),
            body: Box::new(table),
        };
        assert_eq!(
            latex(fenced),
            "\\begin{pmatrix}1 & 2 \\\\ 3 & 4\\end{pmatrix}"
        );
    }

    #[test]
    fn operators_and_functions_map_to_commands() {
        assert_eq!(latex(op("≤")), "\\leq");
        assert_eq!(latex(op("×")), "\\times");
        assert_eq!(latex(op("∞")), "\\infty");
        // A known function identifier becomes its control word.
        assert_eq!(latex(id("sin")), "\\sin");
        // Greek passes through (MathJax renders Unicode).
        assert_eq!(latex(id("ω")), "ω");
        // A plain fenced group uses \left…\right.
        assert_eq!(
            latex(MathExpr::Fenced {
                open: "(".into(),
                close: ")".into(),
                body: Box::new(id("x")),
            }),
            "\\left(x\\right)"
        );
    }

    #[test]
    fn text_token_is_wrapped() {
        assert_eq!(
            latex(MathExpr::Token {
                kind: TokenKind::Text,
                text: "if".into(),
            }),
            "\\text{if}"
        );
    }
}
