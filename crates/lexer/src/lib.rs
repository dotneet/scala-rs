//! Scala 2.13 lexer.
//!
//! Newline tokens are emitted; the parser decides which are statement separators
//! (semicolon inference). Interpolated strings use a mode stack so `${...}` holes
//! are tokenized as ordinary Scala.

mod token;
mod unicode_symbols;

pub use token::{is_operator_name, keyword_kind, Token, TokenKind};

use scala_rs_span::{Diagnostic, SourceFile, Span};

pub fn tokenize(source: &SourceFile, file_index: usize) -> (Vec<Token>, Vec<Diagnostic>) {
    tokenize_opts(source, file_index, false)
}

/// `unicode_escapes_raw` is `-Xsource-features:unicode-escapes-raw`: leave
/// unicode escapes in triple-quoted strings and `raw` interpolations alone.
pub fn tokenize_opts(
    source: &SourceFile,
    file_index: usize,
    unicode_escapes_raw: bool,
) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer::new(source, file_index);
    lx.unicode_escapes_raw = unicode_escapes_raw;
    lx.tokenize_all();
    let tokens = drop_semi_before_else(lx.tokens);
    let tokens = drop_trailing_commas(drop_non_separating_newlines(tokens));
    (tokens, lx.diags)
}

/// nsc `Scanners.postProcessToken`: `SEMI` followed by `ELSE` is `ELSE`, so
/// `if (c) a; else b` (and a `;` ending the line before `else`) is one `if`.
fn drop_semi_before_else(tokens: Vec<Token>) -> Vec<Token> {
    let drop: Vec<bool> = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            matches!(t.kind, TokenKind::Semi)
                && tokens[i + 1..]
                    .iter()
                    .find(|n| !matches!(n.kind, TokenKind::Newline))
                    .is_some_and(|n| matches!(n.kind, TokenKind::Else))
        })
        .collect();
    tokens
        .into_iter()
        .zip(drop)
        .filter_map(|(t, d)| (!d).then_some(t))
        .collect()
}

/// SIP-27 (nsc `Scanners.skipTrailingComma`, called by `inGroupers` and
/// `tokenSeparated`): a comma followed by a line break and then the `)` or `]`
/// closing the innermost group is dropped, wherever the group is -- argument
/// and parameter lists, tuples, type arguments and parameters, patterns, and
/// the single parenthesized expression, where `(23,\n)` is `23`. On one line
/// (`f(a, )`) it is kept and the parser rejects it.
///
/// A `}` is not handled here: in a block a comma is an error before nsc ever
/// asks, so only the comma-separated lists in braces (import selectors) accept
/// one, and the parser checks those itself with [`Token::nl_before`].
fn drop_trailing_commas(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut it = tokens.into_iter().peekable();
    while let Some(t) = it.next() {
        if matches!(t.kind, TokenKind::Comma) {
            if let Some(next) = it.peek() {
                if next.nl_before && matches!(next.kind, TokenKind::RParen | TokenKind::RBracket) {
                    continue;
                }
            }
        }
        out.push(t);
    }
    out
}

/// nsc `Scanners`: a line break separates statements only when the token before
/// it can end one and the token after it can begin one. Without this a chain
/// written as `xs\n  .map(f)` is read as two statements.
fn drop_non_separating_newlines(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut regions: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        match tokens[i].kind {
            TokenKind::LParen => regions.push('('),
            TokenKind::LBracket => regions.push('['),
            TokenKind::LBrace => regions.push('{'),
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                regions.pop();
            }
            _ => {}
        }
        if !matches!(tokens[i].kind, TokenKind::Newline) {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }
        // nsc only inserts newlines in brace regions; inside `(` or `[` a line
        // break never separates. Record it so SIP-27's trailing comma (which
        // requires the newline) can still be told apart.
        if matches!(regions.last(), Some('(') | Some('[')) {
            let mut j = i;
            while j < tokens.len() && matches!(tokens[j].kind, TokenKind::Newline) {
                j += 1;
            }
            if let Some(t) = tokens.get(j) {
                let mut t = t.clone();
                t.nl_before = true;
                // Push the following token here and skip it in the main loop.
                match t.kind {
                    TokenKind::LParen => regions.push('('),
                    TokenKind::LBracket => regions.push('['),
                    TokenKind::LBrace => regions.push('{'),
                    TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                        regions.pop();
                    }
                    _ => {}
                }
                out.push(t);
            }
            i = j + 1;
            continue;
        }
        let mut j = i;
        while j < tokens.len() && matches!(tokens[j].kind, TokenKind::Newline) {
            j += 1;
        }
        let before = out
            .iter()
            .rev()
            .find(|t| !matches!(t.kind, TokenKind::Newline));
        let after = tokens.get(j);
        let keep = before.is_some_and(|t| can_end_statement(&t.kind))
            && after.is_some_and(|t| can_begin_statement(&t.kind));
        if keep {
            // The run collapses to one token, so the `NEWLINES` marker has to
            // come with it: the flag sits on the *second* break of a blank
            // line, and only the first is kept.
            let mut t = tokens[i].clone();
            t.blank_line = tokens[i..j].iter().any(|t| t.blank_line);
            out.push(t);
            i = j;
        } else if let Some(t) = tokens.get(j) {
            // Dropped here too, so keep the fact on the token after it: a
            // trailing comma in an import selector list needs it.
            let mut t = t.clone();
            t.nl_before = true;
            match t.kind {
                TokenKind::LParen => regions.push('('),
                TokenKind::LBracket => regions.push('['),
                TokenKind::LBrace => regions.push('{'),
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    regions.pop();
                }
                _ => {}
            }
            out.push(t);
            i = j + 1;
        } else {
            i = j;
        }
    }
    out
}

/// nsc `inLastOfStat`.
fn can_end_statement(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Ident(_)
            | TokenKind::IntLit(_)
            | TokenKind::LongLit(_)
            | TokenKind::FloatLit(_)
            | TokenKind::DoubleLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::StringLit(_)
            | TokenKind::InterpEnd(_)
            | TokenKind::SymbolLit(_)
            | TokenKind::This
            | TokenKind::Null
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Return
            | TokenKind::Underscore
            | TokenKind::TypeKw
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
    )
}

/// nsc `inFirstOfStat`: these cannot start a statement, so a line break before
/// one of them is not a separator.
fn can_begin_statement(k: &TokenKind) -> bool {
    !matches!(
        k,
        TokenKind::Eof
            | TokenKind::Catch
            | TokenKind::Else
            | TokenKind::Extends
            | TokenKind::Finally
            | TokenKind::ForSome
            | TokenKind::Match
            | TokenKind::With
            | TokenKind::Yield
            | TokenKind::Comma
            | TokenKind::Semi
            | TokenKind::Newline
            | TokenKind::Dot
            | TokenKind::Colon
            | TokenKind::Equals
            | TokenKind::Arrow
            | TokenKind::LeftArrow
            | TokenKind::Subtype
            | TokenKind::Supertype
            | TokenKind::ViewBound
            | TokenKind::Hash
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
            | TokenKind::LBracket
    )
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    file_index: usize,
    pos: usize,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
    /// Brace depth in normal mode; used for interpolation holes.
    brace_depth: i32,
    interp_stack: Vec<InterpFrame>,
    /// `-Xsource-features:unicode-escapes-raw`.
    unicode_escapes_raw: bool,
}

/// What an interpolator does with a backslash in its literal parts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InterpEscapes {
    /// `s` and `f`: `StringContext.processEscapes`, for single- and
    /// triple-quoted literals alike.
    Standard,
    /// `raw`: unicode escapes alone (`StringContext.processUnicode`, which
    /// nsc's `FastStringInterpolator` applies, deprecated since 2.13.2).
    Unicode,
    /// Any other interpolator: the parts verbatim.
    Verbatim,
}

struct InterpFrame {
    triple: bool,
    /// Brace depth at which the `${` hole started; -1 if currently in the string part.
    hole_brace: i32,
    escapes: InterpEscapes,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a SourceFile, file_index: usize) -> Self {
        Lexer {
            src: &source.src,
            bytes: source.src.as_bytes(),
            file_index,
            pos: 0,
            tokens: Vec::new(),
            diags: Vec::new(),
            brace_depth: 0,
            interp_stack: Vec::new(),
            unicode_escapes_raw: false,
        }
    }

    fn tokenize_all(&mut self) {
        while self.pos < self.bytes.len() || !self.interp_stack.is_empty() {
            if let Some(frame) = self.interp_stack.last() {
                if frame.hole_brace < 0 {
                    self.lex_interp_string();
                    continue;
                }
            }
            if self.pos >= self.bytes.len() {
                break;
            }
            self.lex_normal();
        }
        self.emit(TokenKind::Eof, self.pos as u32, self.pos as u32);
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.src.get(self.pos + offset..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let mut chs = self.src[self.pos..].chars();
        if let Some(c) = chs.next() {
            self.pos += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn emit(&mut self, kind: TokenKind, lo: u32, hi: u32) {
        let blank_line = matches!(kind, TokenKind::Newline) && self.ends_blank_line(lo);
        self.tokens.push(Token {
            kind,
            span: Span::new(lo, hi),
            nl_before: false,
            blank_line,
        });
    }

    /// nsc `Scanners.pastBlankLine`: the line this `\n` terminates held
    /// nothing but whitespace, so the break is a `NEWLINES` and not a
    /// `NEWLINE`. A comment counts as content, exactly as it does there --
    /// nsc scans the raw characters, and `/` is not whitespace.
    fn ends_blank_line(&self, at: u32) -> bool {
        let bytes = self.src.as_bytes();
        let mut i = at as usize;
        while i > 0 {
            match bytes[i - 1] {
                b' ' | b'\t' | b'\r' | 0x0c => i -= 1,
                b'\n' => return true,
                _ => return false,
            }
        }
        false
    }

    fn error(&mut self, lo: u32, hi: u32, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, Span::new(lo, hi), msg));
    }

    fn lex_normal(&mut self) {
        let Some(c) = self.peek() else { return };
        match c {
            ' ' | '\t' | '\r' | '\u{000C}' => {
                self.bump();
            }
            '\n' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::Newline, lo, self.pos as u32);
            }
            '/' => {
                if self.starts_with("//") {
                    self.skip_line_comment();
                } else if self.starts_with("/*") {
                    self.skip_block_comment();
                } else {
                    self.lex_operator();
                }
            }
            '"' => self.lex_string(None),
            '\'' => self.lex_char_or_symbol(),
            '`' => self.lex_backtick(),
            '0'..='9' => self.lex_number(),
            '(' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::LParen, lo, self.pos as u32);
            }
            ')' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::RParen, lo, self.pos as u32);
            }
            '[' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::LBracket, lo, self.pos as u32);
            }
            ']' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::RBracket, lo, self.pos as u32);
            }
            '{' => {
                let lo = self.pos as u32;
                self.bump();
                self.brace_depth += 1;
                self.emit(TokenKind::LBrace, lo, self.pos as u32);
            }
            '}' => {
                let lo = self.pos as u32;
                // nsc lexes `${` as an ordinary LBRACE, so a hole is a block
                // and its `}` is an ordinary RBRACE. Emitting the pair keeps
                // the line-break filter's brace region right: without it, a
                // hole written inside a `(...)` argument list inherited the
                // paren region and lost the newlines separating its
                // statements.
                let closes_hole = self
                    .interp_stack
                    .last()
                    .is_some_and(|f| f.hole_brace >= 0 && self.brace_depth == f.hole_brace);
                self.bump();
                self.brace_depth -= 1;
                self.emit(TokenKind::RBrace, lo, self.pos as u32);
                if closes_hole {
                    // Back to the string part: `tokenize_all` sees
                    // `hole_brace < 0` and resumes `lex_interp_string`.
                    self.interp_stack.last_mut().unwrap().hole_brace = -1;
                }
            }
            ',' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::Comma, lo, self.pos as u32);
            }
            '.' => {
                // `.5` is not a Scala float; always a dot (or part of op).
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::Dot, lo, self.pos as u32);
            }
            ';' => {
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::Semi, lo, self.pos as u32);
            }
            '_' => {
                // `_` plus op chars is an identifier (`_+`); lone `_` is Underscore.
                let lo = self.pos as u32;
                self.bump();
                if self.peek().is_some_and(is_id_part) || self.peek().is_some_and(is_op_char) {
                    // continue as identifier
                    self.pos = lo as usize;
                    self.lex_ident_or_kw();
                } else {
                    self.emit(TokenKind::Underscore, lo, self.pos as u32);
                }
            }
            '@' => {
                // nsc scans `@` like any operator character and only the
                // lone `@` is the `AT` token: `type @@[T, U]` names a type.
                if self.peek_at(1).is_some_and(is_op_char) {
                    self.lex_operator();
                } else {
                    let lo = self.pos as u32;
                    self.bump();
                    self.emit(TokenKind::At, lo, self.pos as u32);
                }
            }
            '#' => {
                // A lone `#` is the type projection operator; `##` (and any
                // other run of operator characters starting with `#`) is an
                // ordinary operator identifier -- `x.##` is `Any.##`.
                if self.peek_at(1).is_some_and(is_op_char) {
                    self.lex_operator();
                } else {
                    let lo = self.pos as u32;
                    self.bump();
                    self.emit(TokenKind::Hash, lo, self.pos as u32);
                }
            }
            c if is_id_start(c) => self.lex_ident_or_kw(),
            c if is_op_char(c) => self.lex_operator(),
            _ => {
                let lo = self.pos as u32;
                self.bump();
                self.error(lo, self.pos as u32, format!("unexpected character {c:?}"));
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) {
        let lo = self.pos as u32;
        self.bump(); // /
        self.bump(); // *
        let mut depth = 1;
        while depth > 0 {
            if self.pos >= self.bytes.len() {
                self.error(lo, self.pos as u32, "unterminated block comment");
                return;
            }
            if self.starts_with("/*") {
                self.bump();
                self.bump();
                depth += 1;
            } else if self.starts_with("*/") {
                self.bump();
                self.bump();
                depth -= 1;
            } else {
                self.bump();
            }
        }
    }

    fn lex_ident_or_kw(&mut self) {
        let lo = self.pos as u32;
        let start = self.pos;
        if let Some(c) = self.peek() {
            if is_id_start(c) {
                self.bump();
            }
        }
        while self.peek().is_some_and(is_id_part) {
            self.bump();
        }
        // nsc mixed identifier: `foo_=` / `foo_+=` (`idrest` = letters `_` op).
        // A lone `_` must not swallow `:` / `*` / `=>` (`case _: T =>`, `_*`).
        if self
            .src
            .get(start..self.pos)
            .is_some_and(|t| t.len() > 1 && t.ends_with('_'))
        {
            while self.peek().is_some_and(is_op_char) {
                self.bump();
            }
        }
        let text = &self.src[start..self.pos];
        // Interpolator: identifier immediately followed by " or """
        if self.peek() == Some('"') {
            let prefix = text.to_string();
            self.lex_string(Some(prefix));
            return;
        }
        let kind = keyword_kind(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        self.emit(kind, lo, self.pos as u32);
    }

    fn lex_backtick(&mut self) {
        let lo = self.pos as u32;
        self.bump(); // `
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c == '`' {
                self.bump();
                if text.is_empty() {
                    self.error(lo, self.pos as u32, "empty quoted identifier");
                    return;
                }
                self.emit(TokenKind::Ident(text), lo, self.pos as u32);
                return;
            }
            if matches!(c, '\n' | '\r' | '\u{000C}') {
                break;
            }
            if c == '\\' {
                let escape_lo = self.pos as u32;
                self.bump();
                // Backquoted identifiers use nsc's ordinary literal escapes.
                // The interpolated-string dollar extension is not one of them.
                if self.peek() == Some('$') {
                    self.error(escape_lo, self.pos as u32 + 1, "invalid escape \\$");
                    return;
                }
                match self.read_escape(escape_lo) {
                    Some(c) => text.push(c),
                    None => return,
                }
            } else {
                text.push(self.bump().unwrap());
            }
        }
        self.error(lo, self.pos as u32, "unterminated backquoted identifier");
    }

    fn lex_operator(&mut self) {
        let lo = self.pos as u32;
        let start = self.pos;
        // XML comment/CDATA/PI start with `<!` / `<?`. Do not glue those into
        // Scala operators (`<=`, `<<`, `<-` stay intact: next char is not `!`/`?`).
        if self.peek() == Some('<') {
            let next = self.src[self.pos + 1..].chars().next();
            if matches!(next, Some('!' | '?')) {
                self.bump();
                self.emit(TokenKind::Ident("<".into()), lo, self.pos as u32);
                return;
            }
        }
        while self.peek().is_some_and(is_op_char) {
            let sofar = &self.src[start..self.pos];
            // XML closers must not glue to the next tag or entity: `><!--`,
            // `--></`, `?></`, `>&amp;`.
            if sofar.ends_with('>') && matches!(self.peek(), Some('<' | '&')) {
                break;
            }
            // nsc `getOperatorRest` breaks out of the operator on `/` when what
            // follows starts a comment, so `x =>/*c*/ y` is `=>` and a comment
            // and not the operator `=>/*`. Twirl writes exactly that shape
            // (`case _ =>/*75.22*/ {`) in every generated template, where the
            // munched operator turned the case pattern into an infix pattern.
            if !sofar.is_empty()
                && self.peek() == Some('/')
                && matches!(self.peek_at(1), Some('/' | '*'))
            {
                break;
            }
            self.bump();
        }
        let text = &self.src[start..self.pos];
        let kind = match text {
            "=>" | "⇒" => TokenKind::Arrow,
            "<-" | "←" => TokenKind::LeftArrow,
            "<:" => TokenKind::Subtype,
            ">:" => TokenKind::Supertype,
            "<%" => TokenKind::ViewBound,
            "=" => TokenKind::Equals,
            ":" => TokenKind::Colon,
            // Reached when a comment follows directly (`@/**/`).
            "@" => TokenKind::At,
            "#" => TokenKind::Hash,
            _ => TokenKind::Ident(text.to_string()),
        };
        self.emit(kind, lo, self.pos as u32);
    }

    fn lex_number(&mut self) {
        let lo = self.pos as u32;
        let start = self.pos;
        if self.starts_with("0x") || self.starts_with("0X") {
            self.bump();
            self.bump();
            let digits_start = self.pos;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_hexdigit() || c == '_')
            {
                self.bump();
            }
            if self.pos == digits_start {
                self.error(lo, self.pos as u32, "invalid hex literal");
                return;
            }
            let raw: String = self.src[digits_start..self.pos]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            let suffix = self.eat_num_suffix();
            self.emit_int_from_radix(&raw, 16, suffix, lo);
            return;
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.bump();
        }
        let is_float = self.peek() == Some('.')
            && self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
            || self.peek().is_some_and(|c| c == 'e' || c == 'E');
        // `1.foo` is Int then Dot; `1.0` is float.
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.bump();
            }
        }
        if self.peek() == Some('e') || self.peek() == Some('E') {
            self.bump();
            if self.peek() == Some('+') || self.peek() == Some('-') {
                self.bump();
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.bump();
            }
        }
        let suffix = self.eat_num_suffix();
        let raw: String = self.src[start..self.pos]
            .chars()
            .filter(|c| *c != '_' && !matches!(c, 'l' | 'L' | 'f' | 'F' | 'd' | 'D'))
            .collect();
        // suffix already consumed as part of pos; strip from raw using start..before suffix
        let _ = is_float;
        let body: String = self.src[start..self.pos]
            .trim_end_matches(|c: char| matches!(c, 'l' | 'L' | 'f' | 'F' | 'd' | 'D'))
            .chars()
            .filter(|c| *c != '_')
            .collect();
        match suffix {
            // nsc `intVal`: the magnitude is compared against the type's
            // *positive* limit plus one, because the parser may negate the
            // literal (`-9223372036854775808L` is `Long.MinValue`, and the
            // same digits on their own are "integer number too large").
            // `parse_prefix_expr` does that negation, and rejects a bare
            // boundary literal; here the value is stored wrapped.
            NumSuffix::Long => match body.parse::<u64>() {
                Ok(v) if v <= (i64::MAX as u64) + 1 => {
                    self.emit(TokenKind::LongLit(v as i64), lo, self.pos as u32)
                }
                // nsc reports and goes on with a token, so the parser does
                // not cascade on a missing expression.
                _ => {
                    self.error(lo, self.pos as u32, "integer number too large");
                    self.emit(TokenKind::LongLit(0), lo, self.pos as u32);
                }
            },
            NumSuffix::Float => match body.parse::<f32>() {
                Ok(v) => self.emit(TokenKind::FloatLit(v), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "invalid float literal"),
            },
            NumSuffix::DoubleForced | NumSuffix::None
                if body.contains('.')
                    || body.contains('e')
                    || body.contains('E')
                    || matches!(suffix, NumSuffix::DoubleForced) =>
            {
                match body.parse::<f64>() {
                    Ok(v) => self.emit(TokenKind::DoubleLit(v), lo, self.pos as u32),
                    Err(_) => self.error(lo, self.pos as u32, "invalid double literal"),
                }
            }
            // An integer literal with no `L` is an `Int`, however large the
            // expected type is: nsc rejects `val x: Long = 10000000000`.
            NumSuffix::None => match body.parse::<u64>() {
                Ok(v) if v <= (i32::MAX as u64) + 1 => {
                    self.emit(TokenKind::IntLit(v as u32 as i32), lo, self.pos as u32)
                }
                _ => {
                    self.error(lo, self.pos as u32, "integer number too large");
                    self.emit(TokenKind::IntLit(0), lo, self.pos as u32);
                }
            },
            _ => {
                let _ = raw;
                self.error(lo, self.pos as u32, "invalid numeric literal");
            }
        }
    }

    /// A hexadecimal literal spans the *unsigned* range of its type and is read
    /// as two's complement, which a decimal literal does not: nsc types
    /// `0x85ebca6b` as the `Int` `-2048144789` and rejects the decimal
    /// `2246822507` as "integer number too large". Reading the bits as a
    /// positive `i64` and widening to `Long` made cats-kernel's `MurmurHash3`
    /// avalanche step (`h *= 0x85ebca6b` on an `Int` `h`) a `type mismatch;
    /// found: Long  required: Int`.
    fn emit_int_from_radix(&mut self, digits: &str, radix: u32, suffix: NumSuffix, lo: u32) {
        let bits = u64::from_str_radix(digits, radix);
        match suffix {
            NumSuffix::Long => match bits {
                Ok(v) => self.emit(TokenKind::LongLit(v as i64), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "hex literal out of range"),
            },
            NumSuffix::None => match bits {
                Ok(v) if v <= u32::MAX as u64 => {
                    self.emit(TokenKind::IntLit(v as u32 as i32), lo, self.pos as u32)
                }
                // Wider than 32 bits without an `L`: nsc reports
                // `integer number too large`.
                Ok(_) | Err(_) => self.error(lo, self.pos as u32, "hex literal out of range"),
            },
            _ => self.error(lo, self.pos as u32, "invalid suffix on hex literal"),
        }
    }

    fn eat_num_suffix(&mut self) -> NumSuffix {
        match self.peek() {
            Some('l') | Some('L') => {
                self.bump();
                NumSuffix::Long
            }
            Some('f') | Some('F') => {
                self.bump();
                NumSuffix::Float
            }
            Some('d') | Some('D') => {
                self.bump();
                NumSuffix::DoubleForced
            }
            _ => NumSuffix::None,
        }
    }

    fn lex_char_or_symbol(&mut self) {
        let lo = self.pos as u32;
        self.bump(); // '
                     // symbol: 'ident  (deprecated in 2.13 but still lexical)
        if self.peek().is_some_and(is_id_start) {
            // Could still be 'a' char. If next-next is `'`, it's a char.
            let saved = self.pos;
            let c = self.bump();
            if self.peek() == Some('\'') && c.is_some_and(|ch| !is_id_part(ch) || true) {
                // single char then quote — but ident might be one letter.
                // 'a' is char, 'ab is symbol, 'a_b is symbol.
                if !self.peek_at(0).is_some_and(|_| false) {
                    // We consumed one id char. If immediately `'`, it's CharLit.
                    let ch = c.unwrap();
                    self.bump();
                    self.emit(TokenKind::CharLit(ch), lo, self.pos as u32);
                    return;
                }
            }
            self.pos = saved;
            while self.peek().is_some_and(is_id_part) {
                self.bump();
            }
            let text = self.src[saved..self.pos].to_string();
            self.emit(TokenKind::SymbolLit(text), lo, self.pos as u32);
            return;
        }
        if self.peek() == Some('\\') {
            self.bump();
            match self.read_escape(lo) {
                Some(ch) => {
                    if self.peek() == Some('\'') {
                        self.bump();
                        self.emit(TokenKind::CharLit(ch), lo, self.pos as u32);
                    } else {
                        self.error(lo, self.pos as u32, "unterminated character literal");
                    }
                }
                None => {}
            }
            return;
        }
        if let Some(ch) = self.bump() {
            if self.peek() == Some('\'') {
                self.bump();
                self.emit(TokenKind::CharLit(ch), lo, self.pos as u32);
            } else {
                self.error(lo, self.pos as u32, "unterminated character literal");
            }
        } else {
            self.error(lo, self.pos as u32, "unterminated character literal");
        }
    }

    fn lex_string(&mut self, interp_prefix: Option<String>) {
        let lo = self.pos as u32;
        let triple = self.starts_with("\"\"\"");
        if triple {
            self.bump();
            self.bump();
            self.bump();
        } else {
            self.bump(); // "
        }
        if let Some(prefix) = interp_prefix {
            // Only `s` and `f` process escapes; `raw` processes unicode
            // escapes alone, and every other interpolator gets the parts
            // verbatim, which is what `StringContext.parts` holds in scalac.
            let escapes = match prefix.as_str() {
                "s" | "f" => InterpEscapes::Standard,
                "raw" if !self.unicode_escapes_raw => InterpEscapes::Unicode,
                _ => InterpEscapes::Verbatim,
            };
            self.emit(
                TokenKind::InterpStart { prefix, triple },
                lo,
                self.pos as u32,
            );
            self.interp_stack.push(InterpFrame {
                triple,
                hole_brace: -1,
                escapes,
            });
            self.lex_interp_string();
            return;
        }
        if triple {
            if let Some(s) = self.read_triple_string() {
                // nsc `replaceUnicodeEscapesInTriple` (deprecated since 2.13.2,
                // off under `-Xsource-features:unicode-escapes-raw`).
                let s = if self.unicode_escapes_raw {
                    s
                } else {
                    self.process_unicode(s, lo)
                };
                self.emit(TokenKind::StringLit(s), lo, self.pos as u32);
            } else {
                self.error(lo, self.pos as u32, "unterminated triple-quoted string");
            }
        } else if let Some(s) = self.read_single_string() {
            self.emit(TokenKind::StringLit(s), lo, self.pos as u32);
        } else {
            self.error(lo, self.pos as u32, "unterminated string literal");
        }
    }

    fn read_single_string(&mut self) -> Option<String> {
        let mut buf = String::new();
        loop {
            match self.peek()? {
                '"' => {
                    self.bump();
                    return Some(buf);
                }
                '\n' => return None,
                '\\' => {
                    let lo = self.pos as u32;
                    self.bump();
                    buf.push(self.read_escape(lo)?);
                }
                _ => buf.push(self.bump()?),
            }
        }
    }

    fn read_triple_string(&mut self) -> Option<String> {
        let mut buf = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return None;
            }
            if self.starts_with("\"\"\"") {
                self.bump();
                self.bump();
                self.bump();
                // extra quotes are part of the string
                while self.peek() == Some('"') {
                    buf.push('"');
                    self.bump();
                }
                return Some(buf);
            }
            buf.push(self.bump()?);
        }
    }

    fn lex_interp_string(&mut self) {
        let Some(frame) = self.interp_stack.last() else {
            return;
        };
        let triple = frame.triple;
        let lo = self.pos as u32;
        let mut buf = String::new();
        // nsc `unclosedStringLit(seenEscapedQuote)`: say why when a `\"` is
        // what kept the literal open.
        let mut seen_escaped_quote = false;
        loop {
            if self.pos >= self.bytes.len() || (!triple && self.peek() == Some('\n')) {
                let msg = if seen_escaped_quote {
                    "unclosed string literal; note that `\\\"` no longer closes single-quoted interpolated string literals since 2.13.6, you can use a triple-quoted string instead"
                } else {
                    "unterminated interpolated string"
                };
                self.error(lo, self.pos as u32, msg);
                self.interp_stack.pop();
                return;
            }
            if triple && self.starts_with("\"\"\"") {
                self.bump();
                self.bump();
                self.bump();
                while self.peek() == Some('"') {
                    buf.push('"');
                    self.bump();
                }
                let part = self.finish_interp_part(buf, lo);
                self.emit(TokenKind::InterpEnd(part), lo, self.pos as u32);
                self.interp_stack.pop();
                return;
            }
            if !triple && self.peek() == Some('"') {
                self.bump();
                let part = self.finish_interp_part(buf, lo);
                self.emit(TokenKind::InterpEnd(part), lo, self.pos as u32);
                self.interp_stack.pop();
                return;
            }
            if self.peek() == Some('$') {
                // nsc `getStringPart`: `$$` is a `$` and (since 2.13.6) `$"`
                // a `"`, in every interpolator and both quote styles.
                if self.starts_with("$$") || self.starts_with("$\"") {
                    self.bump();
                    buf.push(self.bump().unwrap());
                    continue;
                }
                // `$_` is a wildcard hole, legal only in a pattern
                // (`case s"$_-$x" =>`); the parser says so in an expression.
                if self.starts_with("$_") {
                    let part = self.finish_interp_part(std::mem::take(&mut buf), lo);
                    self.emit(TokenKind::StringPart(part), lo, self.pos as u32);
                    self.bump(); // $
                    let us_lo = self.pos as u32;
                    self.bump(); // _
                    self.emit(TokenKind::Underscore, us_lo, self.pos as u32);
                    return;
                }
                if self.starts_with("${") {
                    let part = self.finish_interp_part(std::mem::take(&mut buf), lo);
                    self.emit(TokenKind::StringPart(part), lo, self.pos as u32);
                    self.bump(); // $
                    let brace_lo = self.pos as u32;
                    self.bump(); // {
                                 // nsc emits LBRACE here and parses the hole as a block.
                    self.brace_depth += 1;
                    self.emit(TokenKind::LBrace, brace_lo, self.pos as u32);
                    let hole_at = self.brace_depth;
                    self.interp_stack.last_mut().unwrap().hole_brace = hole_at;
                    // Statements follow as normal tokens until matching `}`.
                    return;
                }
                // $ident
                let after_dollar = self.pos + 1;
                let rest = &self.src[after_dollar..];
                if let Some(c) = rest.chars().next() {
                    // `$` is a letter in ordinary code but not here: nsc scans
                    // this name with `Character.isUnicodeIdentifier{Start,Part}`
                    // and not `Chars.isIdentifier{Start,Part}`, so `$l$r` is
                    // two holes. slick writes `b"\($l${op}$r\)"`.
                    if is_interp_id_start(c) {
                        let part = self.finish_interp_part(std::mem::take(&mut buf), lo);
                        self.emit(TokenKind::StringPart(part), lo, self.pos as u32);
                        self.bump(); // $
                        let id_lo = self.pos as u32;
                        self.bump();
                        while self.peek().is_some_and(is_interp_id_part) {
                            self.bump();
                        }
                        let name = self.src[id_lo as usize..self.pos].to_string();
                        self.emit(TokenKind::InterpId(name), id_lo, self.pos as u32);
                        return;
                    }
                }
                // Anything else after `$` is nsc's syntax error; the `$` is
                // then kept as a literal and the string goes on.
                let dollar = self.pos as u32;
                buf.push(self.bump().unwrap());
                let what = match self.peek() {
                    Some(c) if !(triple && self.starts_with("\"\"\"")) => format!("${c}"),
                    _ => "$".to_string(),
                };
                self.error(
                    dollar,
                    dollar + 1,
                    format!(
                        "invalid string interpolation {what}, expected: $$, $\", $identifier or ${{expression}}"
                    ),
                );
                continue;
            }
            let escapes = self
                .interp_stack
                .last()
                .map(|f| f.escapes)
                .unwrap_or(InterpEscapes::Verbatim);
            if self.peek() == Some('\\') {
                // A single-quoted `s` / `f` part is unescaped as it is read.
                // A triple-quoted one is read raw -- a backslash does not keep
                // `"""` from closing it -- and unescaped once complete
                // (`finish_interp_part`), which is the order nsc's scanner and
                // `processEscapes` run in.
                if escapes == InterpEscapes::Standard && !triple {
                    let elo = self.pos as u32;
                    self.bump();
                    seen_escaped_quote |= self.peek() == Some('"');
                    match self.read_escape(elo) {
                        Some(ch) => buf.push(ch),
                        None => return,
                    }
                    continue;
                }
                // nsc `getStringPart`: in a single-quoted literal a backslash
                // and a following `"` or `\` are copied as a pair, so `\"`
                // does not close the literal.
                if !triple {
                    buf.push(self.bump().unwrap());
                    if matches!(self.peek(), Some('"') | Some('\\')) {
                        seen_escaped_quote |= self.peek() == Some('"');
                        buf.push(self.bump().unwrap());
                    }
                    continue;
                }
            }
            buf.push(self.bump().unwrap());
        }
    }

    /// A finished literal part of an interpolation, as the interpolator
    /// receives it: `raw` sees its unicode escapes replaced, and a
    /// triple-quoted `s` / `f` part is unescaped (`s"""a\tb"""` holds a tab).
    fn finish_interp_part(&mut self, buf: String, lo: u32) -> String {
        let Some(frame) = self.interp_stack.last() else {
            return buf;
        };
        match (frame.escapes, frame.triple) {
            (InterpEscapes::Unicode, _) => self.process_unicode(buf, lo),
            (InterpEscapes::Standard, true) => self.process_escapes(buf, lo),
            _ => buf,
        }
    }

    /// `StringContext.processEscapes` over a raw part: `\b \t \n \f \r \" \'
    /// \\` and `\uXXXX`; any other backslash is an error, as nsc's
    /// interpolator macro reports it at compile time.
    fn process_escapes(&mut self, s: String, lo: u32) -> String {
        if !s.contains('\\') {
            return s;
        }
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '\\' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let Some(&c) = chars.get(i + 1) else {
                self.error(
                    lo,
                    self.pos as u32,
                    format!(
                        "invalid escape at terminal index {i} in \"{s}\". Use \\\\ for literal \\."
                    ),
                );
                return out;
            };
            let simple = match c {
                'b' => Some('\u{0008}'),
                't' => Some('\t'),
                'n' => Some('\n'),
                'f' => Some('\u{000C}'),
                'r' => Some('\r'),
                '"' => Some('"'),
                '\'' => Some('\''),
                '\\' => Some('\\'),
                _ => None,
            };
            if let Some(ch) = simple {
                out.push(ch);
                i += 2;
                continue;
            }
            if c == 'u' {
                let mut j = i + 1;
                while chars.get(j) == Some(&'u') {
                    j += 1;
                }
                let hex: String = chars.iter().skip(j).take(4).collect();
                let decoded = (hex.len() == 4 && hex.chars().all(|h| h.is_ascii_hexdigit()))
                    .then(|| u32::from_str_radix(&hex, 16).ok())
                    .flatten()
                    .and_then(char::from_u32);
                if let Some(ch) = decoded {
                    out.push(ch);
                    i = j + 4;
                    continue;
                }
                self.error(lo, self.pos as u32, "invalid unicode escape");
                return out;
            }
            self.error(
                lo,
                self.pos as u32,
                format!(
                    "invalid escape '\\{c}' not one of [\\b, \\t, \\n, \\f, \\r, \\\\, \\\", \\', \\uxxxx] at index {i} in \"{s}\". Use \\\\ for literal \\."
                ),
            );
            return out;
        }
        out
    }

    /// `StringContext.processUnicode`: replace each `\uXXXX` (one or more
    /// `u`s) whose backslash is not itself escaped -- an odd run of
    /// backslashes -- and leave every other backslash alone. An escape that
    /// is not four hex digits is an error, as it is in nsc.
    fn process_unicode(&mut self, s: String, lo: u32) -> String {
        if !s.contains("\\u") {
            return s;
        }
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '\\' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let run = chars[i..].iter().take_while(|&&c| c == '\\').count();
            // All but the last backslash of the run are plain characters;
            // the last escapes a `u` only when the run is odd.
            for _ in 0..run - 1 {
                out.push('\\');
            }
            let last = i + run - 1;
            if run % 2 == 1 && chars.get(last + 1) == Some(&'u') {
                let mut j = last + 1;
                while chars.get(j) == Some(&'u') {
                    j += 1;
                }
                let hex: String = chars.iter().skip(j).take(4).collect();
                match (hex.len() == 4)
                    .then(|| u32::from_str_radix(&hex, 16).ok())
                    .flatten()
                    .filter(|_| hex.chars().all(|c| c.is_ascii_hexdigit()))
                    .and_then(char::from_u32)
                {
                    Some(c) => {
                        out.push(c);
                        i = j + 4;
                    }
                    None => {
                        self.error(lo, self.pos as u32, "invalid unicode escape");
                        out.push('\\');
                        i = last + 1;
                    }
                }
            } else {
                out.push('\\');
                i = last + 1;
            }
        }
        out
    }

    fn read_escape(&mut self, lo: u32) -> Option<char> {
        match self.bump() {
            Some('n') => Some('\n'),
            Some('t') => Some('\t'),
            Some('r') => Some('\r'),
            Some('b') => Some('\u{0008}'),
            Some('f') => Some('\u{000C}'),
            Some('\\') => Some('\\'),
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            // `\$` is no escape: nsc reports "invalid escape character" in a
            // literal, and inside an interpolation the backslash stays a
            // backslash and the `$` starts a hole.
            Some('u') => {
                let mut hex = String::new();
                for _ in 0..4 {
                    match self.peek() {
                        Some(c) if c.is_ascii_hexdigit() => {
                            hex.push(c);
                            self.bump();
                        }
                        _ => {
                            self.error(lo, self.pos as u32, "invalid unicode escape");
                            return None;
                        }
                    }
                }
                let cp = u32::from_str_radix(&hex, 16).ok()?;
                char::from_u32(cp).or_else(|| {
                    self.error(lo, self.pos as u32, "invalid unicode code point");
                    None
                })
            }
            // Reported, and the literal read on: the rest of it is still a
            // literal, and giving up here made the closing quote open a new
            // one.
            Some(c) => {
                self.error(lo, self.pos as u32, format!("invalid escape \\{c}"));
                Some(c)
            }
            None => {
                self.error(lo, self.pos as u32, "unterminated escape");
                None
            }
        }
    }
}

enum NumSuffix {
    None,
    Long,
    Float,
    DoubleForced,
}

pub fn is_id_start(c: char) -> bool {
    // nsc treats `$` as a letter (`Chars.isIdentifierStart`), so `ev$1` is one
    // identifier and not an error. Code that spells compiler-generated names
    // out in the source relies on it: cats' checked-in simulacrum output
    // writes `implicit ev$1: Defer[G]`.
    c.is_ascii_alphabetic()
        || c == '_'
        || c == '$'
        || (!c.is_ascii() && c.is_alphabetic() && !is_unicode_symbol(c))
}

pub fn is_id_part(c: char) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}

/// The name in a `s"$name"` hole. nsc scans it with Java's
/// `Character.isUnicodeIdentifierStart`, which -- unlike `Chars` -- does not
/// count `$`, so `s"$a$b"` is two holes and not one name `a$b`.
fn is_interp_id_start(c: char) -> bool {
    c != '$' && is_id_start(c)
}

fn is_interp_id_part(c: char) -> bool {
    c != '$' && is_id_part(c)
}

pub fn is_op_char(c: char) -> bool {
    matches!(
        c,
        '!' | '#'
            | '%'
            | '&'
            | '*'
            | '+'
            | '-'
            | '/'
            | ':'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '\\'
            | '^'
            | '|'
            | '~'
    ) || (!c.is_ascii() && is_unicode_op(c))
}

/// nsc `Chars.isSpecial`: a Unicode math (Sm) or other (So) symbol is an
/// operator character (SLS 1.1) -- `↑`, `☀`, and supplementary ones like
/// `🌀` too. The table is JDK 17's classification, which is what 2.13 on
/// the pinned JDK uses.
fn is_unicode_op(c: char) -> bool {
    is_unicode_symbol(c)
}

pub fn is_unicode_symbol(c: char) -> bool {
    let u = c as u32;
    unicode_symbols::SYMBOL_RANGES
        .binary_search_by(|&(lo, hi)| {
            if hi < u {
                std::cmp::Ordering::Less
            } else if lo > u {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scala_rs_span::SourceFile;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let sf = SourceFile::new("t.scala", src);
        let (toks, diags) = tokenize(&sf, 0);
        assert!(diags.is_empty(), "{diags:?} for {src:?}");
        toks.into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof | TokenKind::Newline))
            .collect()
    }

    #[test]
    fn keywords_and_idents() {
        use TokenKind::*;
        assert_eq!(kinds("object Main"), vec![Object, Ident("Main".into())]);
        assert_eq!(
            kinds("try catch finally throw"),
            vec![Try, Catch, Finally, Throw]
        );
    }

    #[test]
    fn integers() {
        use TokenKind::*;
        assert_eq!(
            kinds("1 2L 0x10 1_000"),
            vec![IntLit(1), LongLit(2), IntLit(16), IntLit(1000)]
        );
    }

    /// A hexadecimal literal spans the *unsigned* range of its type and is
    /// read as two's complement, unlike a decimal one: nsc types `0x85ebca6b`
    /// as the `Int` `-2048144789`, `0xffffffff` as `-1`, and
    /// `0xffffffffffffffffL` as `-1L`, while rejecting anything wider than the
    /// suffix allows with `integer number too large`.
    #[test]
    fn hex_literals_wrap_to_their_width() {
        use TokenKind::*;
        assert_eq!(
            kinds("0x85ebca6b 0xffffffff 0x7fffffff 0xffffffffffffffffL"),
            vec![
                IntLit(-2048144789),
                IntLit(-1),
                IntLit(i32::MAX),
                LongLit(-1)
            ]
        );
        let sf = SourceFile::new("t.scala", "0x100000000");
        let (_, diags) = tokenize(&sf, 0);
        assert!(
            !diags.is_empty(),
            "0x100000000 does not fit an Int and has no `L`"
        );
    }

    #[test]
    fn strings() {
        use TokenKind::*;
        assert_eq!(kinds(r#""hello\n""#), vec![StringLit("hello\n".into())]);
        assert_eq!(kinds("\"\"\"a\"b\"\"\""), vec![StringLit("a\"b".into())]);
    }

    #[test]
    fn interpolation() {
        let sf = SourceFile::new("t.scala", r#"s"hi $name""#);
        let (toks, d) = tokenize(&sf, 0);
        assert!(d.is_empty());
        let ks: Vec<_> = toks
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof | TokenKind::Newline))
            .collect();
        assert!(matches!(ks[0], TokenKind::InterpStart { .. }));
        assert!(matches!(&ks[1], TokenKind::StringPart(s) if s == "hi "));
        assert!(matches!(&ks[2], TokenKind::InterpId(s) if s == "name"));
        assert!(matches!(&ks[3], TokenKind::InterpEnd(s) if s.is_empty()));
    }

    #[test]
    fn raw_interpolator_keeps_escapes() {
        let sf = SourceFile::new("t.scala", r#"raw"a\nb""#);
        let (toks, d) = tokenize(&sf, 0);
        assert!(d.is_empty(), "{d:?}");
        let ks: Vec<_> = toks
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof | TokenKind::Newline))
            .collect();
        assert!(
            matches!(&ks[1], TokenKind::InterpEnd(s) if s == r"a\nb"),
            "{ks:?}"
        );
    }

    #[test]
    fn s_interpolator_interprets_escapes() {
        let sf = SourceFile::new("t.scala", r#"s"a\nb""#);
        let (toks, d) = tokenize(&sf, 0);
        assert!(d.is_empty(), "{d:?}");
        let ks: Vec<_> = toks
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof | TokenKind::Newline))
            .collect();
        assert!(
            matches!(&ks[1], TokenKind::InterpEnd(s) if s == "a\nb"),
            "{ks:?}"
        );
    }

    #[test]
    fn comments_nested() {
        assert_eq!(
            kinds("1 /* /* x */ */ 2"),
            vec![TokenKind::IntLit(1), TokenKind::IntLit(2)]
        );
    }

    #[test]
    fn operators() {
        use TokenKind::*;
        assert_eq!(kinds("=> <- <:"), vec![Arrow, LeftArrow, Subtype]);
        assert_eq!(kinds("+ ++"), vec![Ident("+".into()), Ident("++".into())]);
        assert_eq!(kinds("foo_="), vec![Ident("foo_=".into())]);
        assert_eq!(
            kinds("_: T =>"),
            vec![Ident("_".into()), Colon, Ident("T".into()), Arrow]
        );
        assert_eq!(kinds("_*"), vec![Ident("_".into()), Ident("*".into())]);
    }

    #[test]
    fn xml_markup_is_not_glued_to_gt() {
        use TokenKind::*;
        assert_eq!(
            kinds("><!--"),
            vec![Ident(">".into()), Ident("<".into()), Ident("!--".into())]
        );
        assert_eq!(kinds("<!--"), vec![Ident("<".into()), Ident("!--".into())]);
        assert_eq!(kinds("<?"), vec![Ident("<".into()), Ident("?".into())]);
        assert_eq!(kinds("</"), vec![Ident("</".into())]);
        assert_eq!(kinds("/>"), vec![Ident("/>".into())]);
        assert_eq!(
            kinds("--></"),
            vec![Ident("-->".into()), Ident("</".into())]
        );
        assert_eq!(kinds("?></"), vec![Ident("?>".into()), Ident("</".into())]);
        assert_eq!(kinds("<="), vec![Ident("<=".into())]);
        assert_eq!(kinds("<<"), vec![Ident("<<".into())]);
        assert_eq!(kinds("<-"), vec![LeftArrow]);
        assert_eq!(kinds("&#65;"), vec![Ident("&#".into()), IntLit(65), Semi]);
        assert_eq!(
            kinds("&amp;"),
            vec![Ident("&".into()), Ident("amp".into()), Semi]
        );
        assert_eq!(
            kinds(">&amp;"),
            vec![
                Ident(">".into()),
                Ident("&".into()),
                Ident("amp".into()),
                Semi
            ]
        );
    }

    /// SIP-27: only before a line break and the group's `)` / `]`.
    #[test]
    fn trailing_commas() {
        use TokenKind::*;
        assert_eq!(
            kinds("f(a,\n)"),
            vec![Ident("f".into()), LParen, Ident("a".into()), RParen]
        );
        assert_eq!(
            kinds("C[A,\n]"),
            vec![Ident("C".into()), LBracket, Ident("A".into()), RBracket]
        );
        assert_eq!(
            kinds("f(a, )"),
            vec![Ident("f".into()), LParen, Ident("a".into()), Comma, RParen]
        );
        // A brace group keeps it; the parser decides (import selectors).
        assert_eq!(
            kinds("{a,\n}"),
            vec![LBrace, Ident("a".into()), Comma, RBrace]
        );
    }

    /// nsc `postProcessToken`: `SEMI ELSE` is `ELSE`.
    #[test]
    fn semicolon_before_else() {
        use TokenKind::*;
        assert_eq!(
            kinds("if (c) a; else b"),
            vec![
                If,
                LParen,
                Ident("c".into()),
                RParen,
                Ident("a".into()),
                Else,
                Ident("b".into())
            ]
        );
        assert_eq!(kinds("a;\nelse"), vec![Ident("a".into()), Else]);
        assert_eq!(
            kinds("a; b"),
            vec![Ident("a".into()), Semi, Ident("b".into())]
        );
    }

    #[test]
    fn interpolation_dollar_quote_and_wildcard_hole() {
        use TokenKind::*;
        assert_eq!(
            kinds("s\"$\"x$$\""),
            vec![
                InterpStart {
                    prefix: "s".into(),
                    triple: false
                },
                InterpEnd("\"x$".into())
            ]
        );
        assert_eq!(
            kinds("s\"a$_b\""),
            vec![
                InterpStart {
                    prefix: "s".into(),
                    triple: false
                },
                StringPart("a".into()),
                Underscore,
                InterpEnd("b".into())
            ]
        );
        // A raw part keeps `\"` and it does not end the literal.
        assert_eq!(
            kinds("raw\"\\\"a\""),
            vec![
                InterpStart {
                    prefix: "raw".into(),
                    triple: false
                },
                InterpEnd("\\\"a".into())
            ]
        );
        // A triple-quoted `s` part is escape-processed after scanning.
        assert_eq!(
            kinds("s\"\"\"a\\tb\"\"\""),
            vec![
                InterpStart {
                    prefix: "s".into(),
                    triple: true
                },
                InterpEnd("a\tb".into())
            ]
        );
    }

    /// nsc `Chars.isSpecial`: Sm / So code points are operator characters,
    /// supplementary ones included; `@` heads an operator unless alone.
    #[test]
    fn unicode_symbols_and_at_operators() {
        use TokenKind::*;
        assert_eq!(kinds("↑"), vec![Ident("↑".into())]);
        assert_eq!(
            kinds("a ☀= b"),
            vec![Ident("a".into()), Ident("☀=".into()), Ident("b".into())]
        );
        assert_eq!(kinds("🌀d"), vec![Ident("🌀".into()), Ident("d".into())]);
        assert_eq!(kinds("𐀀"), vec![Ident("𐀀".into())]);
        assert_eq!(kinds("@@"), vec![Ident("@@".into())]);
        assert_eq!(
            kinds("x @ y"),
            vec![Ident("x".into()), At, Ident("y".into())]
        );
        assert_eq!(kinds("@tailrec"), vec![At, Ident("tailrec".into())]);
        assert!(!is_unicode_symbol('€'), "Sc is not an operator character");
    }
}
