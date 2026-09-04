//! Scala 2.13 lexer.
//!
//! Newline tokens are emitted; the parser decides which are statement separators
//! (semicolon inference). Interpolated strings use a mode stack so `${...}` holes
//! are tokenized as ordinary Scala.

mod token;

pub use token::{is_operator_name, keyword_kind, Token, TokenKind};

use scala_rs_span::{Diagnostic, SourceFile, Span};

pub fn tokenize(source: &SourceFile, file_index: usize) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer::new(source, file_index);
    lx.tokenize_all();
    let tokens = drop_non_separating_newlines(lx.tokens);
    (tokens, lx.diags)
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
            out.push(tokens[i].clone());
        }
        i = j;
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
}

struct InterpFrame {
    triple: bool,
    /// Brace depth at which the `${` hole started; -1 if currently in the string part.
    hole_brace: i32,
    /// `raw"..."` does not interpret escape sequences (nsc `StringContext.raw`).
    raw: bool,
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
        self.tokens.push(Token {
            kind,
            span: Span::new(lo, hi),
            nl_before: false,
        });
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
                // `${` does not increment `brace_depth`; the matching `}` must
                // therefore close the hole when depth equals `hole_brace`.
                if let Some(frame) = self.interp_stack.last() {
                    if frame.hole_brace >= 0 && self.brace_depth == frame.hole_brace {
                        self.bump();
                        self.interp_stack.last_mut().unwrap().hole_brace = -1;
                        return;
                    }
                }
                self.bump();
                self.brace_depth -= 1;
                self.emit(TokenKind::RBrace, lo, self.pos as u32);
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
                let lo = self.pos as u32;
                self.bump();
                self.emit(TokenKind::At, lo, self.pos as u32);
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
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '`' {
                let text = self.src[start..self.pos].to_string();
                self.bump();
                self.emit(TokenKind::Ident(text), lo, self.pos as u32);
                return;
            }
            if c == '\n' {
                break;
            }
            self.bump();
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
            NumSuffix::Long => match body.parse::<i64>() {
                Ok(v) => self.emit(TokenKind::LongLit(v), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "integer literal out of range"),
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
            NumSuffix::None => match body.parse::<i64>() {
                Ok(v) if v >= i32::MIN as i64 && v <= i32::MAX as i64 => {
                    self.emit(TokenKind::IntLit(v as i32), lo, self.pos as u32)
                }
                Ok(v) => self.emit(TokenKind::LongLit(v), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "integer literal out of range"),
            },
            _ => {
                let _ = raw;
                self.error(lo, self.pos as u32, "invalid numeric literal");
            }
        }
    }

    fn emit_int_from_radix(&mut self, digits: &str, radix: u32, suffix: NumSuffix, lo: u32) {
        match suffix {
            NumSuffix::Long => match i64::from_str_radix(digits, radix) {
                Ok(v) => self.emit(TokenKind::LongLit(v), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "hex literal out of range"),
            },
            NumSuffix::None => match i64::from_str_radix(digits, radix) {
                Ok(v) if v >= i32::MIN as i64 && v <= i32::MAX as i64 => {
                    self.emit(TokenKind::IntLit(v as i32), lo, self.pos as u32)
                }
                Ok(v) => self.emit(TokenKind::LongLit(v), lo, self.pos as u32),
                Err(_) => self.error(lo, self.pos as u32, "hex literal out of range"),
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
            // Only `s` and `f` process escapes; every other interpolator —
            // `raw` and any user-defined one — gets the parts verbatim, which
            // is what `StringContext.parts` holds in scalac.
            let raw = !matches!(prefix.as_str(), "s" | "f");
            self.emit(
                TokenKind::InterpStart { prefix, triple },
                lo,
                self.pos as u32,
            );
            self.interp_stack.push(InterpFrame {
                triple,
                hole_brace: -1,
                raw,
            });
            self.lex_interp_string();
            return;
        }
        if triple {
            if let Some(s) = self.read_triple_string() {
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
        loop {
            if self.pos >= self.bytes.len() {
                self.error(lo, self.pos as u32, "unterminated interpolated string");
                self.interp_stack.pop();
                return;
            }
            if !triple && self.peek() == Some('\n') {
                self.error(lo, self.pos as u32, "unterminated interpolated string");
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
                self.emit(TokenKind::InterpEnd(buf), lo, self.pos as u32);
                self.interp_stack.pop();
                return;
            }
            if !triple && self.peek() == Some('"') {
                self.bump();
                self.emit(TokenKind::InterpEnd(buf), lo, self.pos as u32);
                self.interp_stack.pop();
                return;
            }
            if self.peek() == Some('$') {
                if self.starts_with("$$") {
                    self.bump();
                    self.bump();
                    buf.push('$');
                    continue;
                }
                if self.starts_with("${") {
                    self.emit(
                        TokenKind::StringPart(std::mem::take(&mut buf)),
                        lo,
                        self.pos as u32,
                    );
                    self.bump(); // $
                    self.bump(); // {
                    let hole_at = self.brace_depth;
                    self.interp_stack.last_mut().unwrap().hole_brace = hole_at;
                    // Expressions follow as normal tokens until matching `}`.
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
                        self.emit(
                            TokenKind::StringPart(std::mem::take(&mut buf)),
                            lo,
                            self.pos as u32,
                        );
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
                // stray $
                buf.push(self.bump().unwrap());
                continue;
            }
            let raw = self.interp_stack.last().map(|f| f.raw).unwrap_or(false);
            if !triple && !raw && self.peek() == Some('\\') {
                let elo = self.pos as u32;
                self.bump();
                match self.read_escape(elo) {
                    Some(ch) => buf.push(ch),
                    None => return,
                }
                continue;
            }
            buf.push(self.bump().unwrap());
        }
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
            Some('$') => Some('$'),
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
            Some(c) => {
                self.error(lo, self.pos as u32, format!("invalid escape \\{c}"));
                None
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
    c.is_ascii_alphabetic() || c == '_' || c == '$' || (!c.is_ascii() && c.is_alphabetic())
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

fn is_unicode_op(c: char) -> bool {
    matches!(c, '⇒' | '←' | '→') || {
        let u = c as u32;
        // Sm, So — approximation of Scala's op chars
        matches!(c, '∀'..='⋿') || (0x2200..=0x22FF).contains(&u)
    }
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
}
