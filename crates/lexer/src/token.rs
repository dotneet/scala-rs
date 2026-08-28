use scala_rs_span::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// A line break preceded this token but did not separate statements.
    /// SIP-27's trailing comma needs it (`f(a, b,\n)` is legal, `f(a, b,)` is not).
    pub nl_before: bool,
}

impl Token {
    pub fn is_nl_or_semi(&self) -> bool {
        matches!(self.kind, TokenKind::Newline | TokenKind::Semi)
    }

    pub fn ident_text(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(s) => Some(s.as_str()),
            TokenKind::InterpId(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn is_ident(&self) -> bool {
        matches!(self.kind, TokenKind::Ident(_))
    }

    pub fn is_operator_ident(&self) -> bool {
        match &self.kind {
            TokenKind::Ident(s) => is_operator_name(s),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Eof,
    Newline,
    Semi,
    Comma,
    Dot,
    Colon,
    Equals,
    Arrow,
    LeftArrow,
    Subtype,
    Supertype,
    ViewBound,
    Hash,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Underscore,
    Ident(String),
    IntLit(i32),
    LongLit(i64),
    DoubleLit(f64),
    FloatLit(f32),
    CharLit(char),
    StringLit(String),
    SymbolLit(String),
    True,
    False,
    Null,
    Abstract,
    Case,
    Catch,
    Class,
    Def,
    Do,
    Else,
    Extends,
    Final,
    Finally,
    For,
    ForSome,
    If,
    Implicit,
    Import,
    Lazy,
    Macro,
    Match,
    New,
    Object,
    Override,
    Package,
    Private,
    Protected,
    Return,
    Sealed,
    Super,
    This,
    Throw,
    Trait,
    Try,
    TypeKw,
    Val,
    Var,
    While,
    With,
    Yield,
    InterpStart { prefix: String, triple: bool },
    StringPart(String),
    InterpId(String),
    InterpEnd(String),
}

impl TokenKind {
    pub fn is_eof(&self) -> bool {
        matches!(self, TokenKind::Eof)
    }

    pub fn can_end_stat(&self) -> bool {
        matches!(
            self,
            TokenKind::Ident(_)
                | TokenKind::IntLit(_)
                | TokenKind::LongLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::CharLit(_)
                | TokenKind::StringLit(_)
                | TokenKind::SymbolLit(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
                | TokenKind::This
                | TokenKind::Super
                | TokenKind::RParen
                | TokenKind::RBrace
                | TokenKind::RBracket
                | TokenKind::Underscore
                | TokenKind::Return
                | TokenKind::InterpEnd(_)
                | TokenKind::TypeKw
        )
    }

    pub fn can_start_stat(&self) -> bool {
        !matches!(
            self,
            TokenKind::Catch
                | TokenKind::Else
                | TokenKind::Extends
                | TokenKind::Finally
                | TokenKind::Match
                | TokenKind::With
                | TokenKind::Yield
                | TokenKind::Comma
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
                | TokenKind::Semi
                | TokenKind::Eof
                | TokenKind::Case
        )
    }

    pub fn as_ident(&self) -> Option<&str> {
        match self {
            TokenKind::Ident(s) => Some(s),
            _ => None,
        }
    }
}

pub fn is_operator_name(s: &str) -> bool {
    let Some(c) = s.chars().next() else {
        return false;
    };
    crate::is_op_char(c)
}

pub fn keyword_kind(text: &str) -> Option<TokenKind> {
    Some(match text {
        "abstract" => TokenKind::Abstract,
        "case" => TokenKind::Case,
        "catch" => TokenKind::Catch,
        "class" => TokenKind::Class,
        "def" => TokenKind::Def,
        "do" => TokenKind::Do,
        "else" => TokenKind::Else,
        "extends" => TokenKind::Extends,
        "false" => TokenKind::False,
        "final" => TokenKind::Final,
        "finally" => TokenKind::Finally,
        "for" => TokenKind::For,
        "forSome" => TokenKind::ForSome,
        "if" => TokenKind::If,
        "implicit" => TokenKind::Implicit,
        "import" => TokenKind::Import,
        "lazy" => TokenKind::Lazy,
        "macro" => TokenKind::Macro,
        "match" => TokenKind::Match,
        "new" => TokenKind::New,
        "null" => TokenKind::Null,
        "object" => TokenKind::Object,
        "override" => TokenKind::Override,
        "package" => TokenKind::Package,
        "private" => TokenKind::Private,
        "protected" => TokenKind::Protected,
        "return" => TokenKind::Return,
        "sealed" => TokenKind::Sealed,
        "super" => TokenKind::Super,
        "this" => TokenKind::This,
        "throw" => TokenKind::Throw,
        "trait" => TokenKind::Trait,
        "true" => TokenKind::True,
        "try" => TokenKind::Try,
        "type" => TokenKind::TypeKw,
        "val" => TokenKind::Val,
        "var" => TokenKind::Var,
        "while" => TokenKind::While,
        "with" => TokenKind::With,
        "yield" => TokenKind::Yield,
        _ => return None,
    })
}
