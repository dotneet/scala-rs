#![allow(dead_code)]
//! Recursive-descent parser for a Scala 2.13 subset.
//! Unimplemented constructs produce `TreeKind::Unimplemented` plus a diagnostic
//! rather than being dropped.

use crate::ast::*;
use scala_rs_lexer::{is_operator_name, Token, TokenKind};
use scala_rs_span::{Diagnostic, SourceFile, Span};

pub struct ParseResult {
    pub tree: Tree,
    pub diags: Vec<Diagnostic>,
}

pub fn parse_source(source: &SourceFile, file_index: usize, tokens: Vec<Token>) -> ParseResult {
    let mut p = Parser::new(source, file_index, tokens);
    let tree = p.parse_compilation_unit();
    ParseResult {
        tree,
        diags: p.diags,
    }
}

/// Compiler annotations this subset does not implement. User-defined
/// `StaticAnnotation` classes (`@Ann(foo)`) are accepted so we can pickle them.
fn annotation_compiler_unsupported(path: &str) -> bool {
    let simple = path.rsplit('.').next().unwrap_or(path);
    matches!(
        simple,
        "specialized" | "unspecialized" | "elidable" | "strictfp"
    )
}

fn annotation_supported(path: &str) -> bool {
    !annotation_compiler_unsupported(path)
}

/// XML attribute: unprefixed `b={e}` or prefixed `p:b={e}`.
struct XmlAttr {
    prefix: Option<String>,
    key: String,
    value: Tree,
}

struct Parser<'a> {
    source: &'a SourceFile,
    file_index: usize,
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    next_id: u32,
    /// nsc `placeholderParams`: synthetic vals from expression `_`, newest last.
    placeholder_params: Vec<Tree>,
    /// Inside a typed pattern, `|` starts the next alternative, not an infix type.
    no_bar_infix_type: bool,
    placeholder_id: u32,
}

impl<'a> Parser<'a> {
    fn new(source: &'a SourceFile, file_index: usize, tokens: Vec<Token>) -> Self {
        Parser {
            source,
            file_index,
            tokens,
            pos: 0,
            diags: Vec::new(),
            next_id: 1,
            no_bar_infix_type: false,
            placeholder_params: Vec::new(),
            placeholder_id: 0,
        }
    }

    fn alloc(&mut self, span: Span, kind: TreeKind) -> Tree {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Tree::new(id, span, kind)
    }

    fn empty(&mut self, span: Span) -> Tree {
        self.alloc(span, TreeKind::Empty)
    }

    /// nsc `withPlaceholders`: wrap a non-bare `_` section as `Function`.
    fn with_placeholders(&mut self, f: impl FnOnce(&mut Self) -> Tree) -> Tree {
        let saved = std::mem::take(&mut self.placeholder_params);
        let mut res = f(self);
        if !self.placeholder_params.is_empty()
            && !is_placeholder_wildcard(&res, &self.placeholder_params)
        {
            let params = std::mem::take(&mut self.placeholder_params);
            let span = res.span;
            res = self.alloc(
                span,
                TreeKind::Function {
                    vparams: params,
                    body: Box::new(res),
                },
            );
        }
        let mut leftover = std::mem::take(&mut self.placeholder_params);
        leftover.extend(saved);
        self.placeholder_params = leftover;
        res
    }

    fn finish_no_escaping(&mut self, saved: Vec<Tree>) {
        if let Some(p) = self.placeholder_params.first() {
            let sp = p.span;
            self.error_span(sp, "unbound placeholder parameter");
            self.placeholder_params.clear();
        }
        self.placeholder_params = saved;
    }

    fn fresh_placeholder(&mut self) -> Tree {
        let sp = self.span();
        self.bump(); // Underscore, or Ident("_") before `:` / `*`
        self.placeholder_id += 1;
        let name = format!("x${}", self.placeholder_id);
        let id = self.alloc(sp, TreeKind::Ident { name: name.clone() });
        let empty_tpt = self.empty(sp);
        let empty_rhs = self.empty(sp);
        let param = self.alloc(
            sp,
            TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM.with(Flags::SYNTHETIC)),
                name,
                tpt: Box::new(empty_tpt),
                rhs: Box::new(empty_rhs),
            },
        );
        self.placeholder_params.push(param);
        id
    }

    fn remove_placeholder_named(&mut self, name: &str) {
        self.placeholder_params.retain(|p| p.name() != Some(name));
    }

    fn convert_to_params(&mut self, t: Tree) -> Vec<Tree> {
        fn names_in(t: &Tree) -> Vec<String> {
            match &t.kind {
                TreeKind::Ident { name } => vec![name.clone()],
                TreeKind::Typed { expr, .. } => names_in(expr),
                TreeKind::Apply { args, .. } => args.iter().flat_map(names_in).collect(),
                _ => vec![],
            }
        }
        for n in names_in(&t) {
            self.remove_placeholder_named(&n);
        }
        expr_to_params(t)
    }

    fn tok(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| self.tokens.last().unwrap())
    }

    fn kind(&self) -> &TokenKind {
        &self.tok().kind
    }

    fn span(&self) -> Span {
        self.tok().span
    }

    fn at_eof(&self) -> bool {
        matches!(self.kind(), TokenKind::Eof)
    }

    fn bump(&mut self) -> Token {
        let t = self.tok().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn skip_nl(&mut self) {
        while matches!(self.kind(), TokenKind::Newline) {
            self.bump();
        }
    }

    fn skip_nl_semi(&mut self) {
        while matches!(self.kind(), TokenKind::Newline | TokenKind::Semi) {
            self.bump();
        }
    }

    fn peek_non_nl(&self) -> &TokenKind {
        let mut i = self.pos;
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        self.tokens
            .get(i)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn at(&self, pred: impl Fn(&TokenKind) -> bool) -> bool {
        pred(self.kind())
    }

    fn eat(&mut self, pred: impl Fn(&TokenKind) -> bool) -> bool {
        if pred(self.kind()) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, what: &str, pred: impl Fn(&TokenKind) -> bool) -> Span {
        self.skip_nl();
        let sp = self.span();
        if pred(self.kind()) {
            self.bump();
            sp
        } else {
            self.error_here(format!(
                "expected {what}, found {}",
                token_name(self.kind())
            ));
            sp
        }
    }

    fn error_here(&mut self, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, self.span(), msg));
    }

    fn error_span(&mut self, span: Span, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, span, msg));
    }

    fn unimplemented(&mut self, span: Span, what: impl Into<String>) -> Tree {
        let what = what.into();
        self.error_span(span, format!("unimplemented syntax: {what}"));
        self.alloc(span, TreeKind::Unimplemented { what })
    }

    fn ident_text(&self) -> Option<String> {
        match self.kind() {
            TokenKind::Ident(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn expect_ident(&mut self) -> (String, Span) {
        self.skip_nl();
        let sp = self.span();
        match self.kind().clone() {
            TokenKind::Ident(s) => {
                self.bump();
                (s, sp)
            }
            TokenKind::This => {
                self.bump();
                ("this".into(), sp)
            }
            _ => {
                self.error_here("expected identifier");
                ("<error>".into(), sp)
            }
        }
    }

    // ------------------------------------------------------------------
    // Compilation unit
    // ------------------------------------------------------------------

    fn parse_compilation_unit(&mut self) -> Tree {
        self.skip_nl_semi();
        let lo = self.span();
        let mut pid = None;
        if matches!(self.kind(), TokenKind::Package) {
            // Could be `package object` or `package qual { }` or leading package clause.
            let pkg_span = self.span();
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Object) {
                let pkg_obj = self.parse_package_object(pkg_span);
                self.skip_nl_semi();
                let rest = self.parse_top_stats();
                let mut all = vec![pkg_obj];
                all.extend(rest);
                let pid = self.alloc(
                    pkg_span,
                    TreeKind::Ident {
                        name: "_root_".into(),
                    },
                );
                return self.alloc(
                    lo.merge(self.prev_span()),
                    TreeKind::PackageDef {
                        pid: Box::new(pid),
                        stats: all,
                    },
                );
            }
            pid = Some(self.parse_path());
            // `package p { stats }` vs `package p; stats`
            self.skip_nl();
            if matches!(self.kind(), TokenKind::LBrace) {
                self.bump();
                let stats = self.parse_top_stats();
                self.expect("}", |k| matches!(k, TokenKind::RBrace));
                let mut more = self.parse_top_stats();
                let mut stats = stats;
                stats.append(&mut more);
                return self.alloc(
                    lo.merge(self.prev_span()),
                    TreeKind::PackageDef {
                        pid: Box::new(pid.unwrap()),
                        stats,
                    },
                );
            }
            self.accept_separator();
        }
        let stats = self.parse_top_stats();
        let pid = pid.unwrap_or_else(|| {
            self.alloc(
                lo,
                TreeKind::Ident {
                    name: "<empty>".into(),
                },
            )
        });
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::PackageDef {
                pid: Box::new(pid),
                stats,
            },
        )
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            self.span()
        } else {
            self.tokens[self.pos.saturating_sub(1)].span
        }
    }

    fn accept_separator(&mut self) -> bool {
        if matches!(self.kind(), TokenKind::Semi | TokenKind::Newline) {
            while matches!(self.kind(), TokenKind::Semi | TokenKind::Newline) {
                self.bump();
            }
            true
        } else {
            false
        }
    }

    fn parse_top_stats(&mut self) -> Vec<Tree> {
        let saved = std::mem::take(&mut self.placeholder_params);
        let mut stats = Vec::new();
        loop {
            self.skip_nl_semi();
            if self.at_eof() || matches!(self.kind(), TokenKind::RBrace) {
                break;
            }
            if matches!(self.kind(), TokenKind::Package) {
                let lo = self.span();
                self.bump();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::Object) {
                    stats.push(self.parse_package_object(lo));
                } else {
                    let pid = self.parse_path();
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::LBrace) {
                        self.bump();
                        let inner = self.parse_top_stats();
                        self.expect("}", |k| matches!(k, TokenKind::RBrace));
                        stats.push(self.alloc(
                            lo.merge(self.prev_span()),
                            TreeKind::PackageDef {
                                pid: Box::new(pid),
                                stats: inner,
                            },
                        ));
                    } else {
                        self.error_here("package clause inside a unit must be followed by { ... }");
                    }
                }
                continue;
            }
            if matches!(self.kind(), TokenKind::Import) {
                stats.push(self.parse_import());
                continue;
            }
            stats.extend(flatten_val_block(self.parse_tmpl_or_def()));
            if !self.accept_separator()
                && !self.at_eof()
                && !matches!(self.kind(), TokenKind::RBrace)
            {
                // Adjacent template defs without separator — still ok if next is a keyword.
                if !is_mod_or_def_start(self.kind()) {
                    self.error_here("expected newline or `;` after statement");
                    self.bump();
                }
            }
        }
        self.finish_no_escaping(saved);
        stats
    }

    fn parse_import(&mut self) -> Tree {
        let lo = self.span();
        self.bump(); // import
        self.skip_nl();
        let expr = self.parse_import_expr();
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::Import {
                expr: Box::new(expr),
                selectors: vec![], // folded into expr via Select / braces
            },
        )
    }

    /// `foo.bar`, `foo.bar.Baz`, `foo.{A, B => C, _}`
    fn parse_import_expr(&mut self) -> Tree {
        let mut t = self.parse_ident_tree();
        loop {
            self.skip_nl();
            if !matches!(self.kind(), TokenKind::Dot) {
                break;
            }
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Underscore) {
                let sp = self.span();
                self.bump();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::Select {
                        qual: Box::new(t),
                        name: "_".into(),
                    },
                );
                break;
            }
            if matches!(self.kind(), TokenKind::LBrace) {
                let sels = self.parse_import_selectors();
                // Encode selectors on a synthetic Select name `{...}` and stash
                // them by wrapping Import at the caller... we attach via a Block
                // of Ident trees named "sel:rename".
                let mut names = Vec::new();
                for s in &sels {
                    let n = match &s.rename {
                        Some(r) => format!("{}=>{}", s.name, r),
                        None => s.name.clone(),
                    };
                    names.push(n);
                }
                t = self.alloc(
                    t.span.merge(self.prev_span()),
                    TreeKind::Select {
                        qual: Box::new(t),
                        name: format!("{{{}}}", names.join(",")),
                    },
                );
                break;
            }
            let (name, sp) = self.expect_ident();
            t = self.alloc(
                t.span.merge(sp),
                TreeKind::Select {
                    qual: Box::new(t),
                    name,
                },
            );
        }
        t
    }

    fn parse_import_selectors(&mut self) -> Vec<ImportSelector> {
        self.bump(); // {
        let mut sels = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::RBrace) {
                self.bump();
                break;
            }
            let sp = self.span();
            if matches!(self.kind(), TokenKind::Underscore) {
                self.bump();
                sels.push(ImportSelector::wildcard(sp));
            } else {
                let (name, nsp) = self.expect_ident();
                self.skip_nl();
                let rename = if matches!(self.kind(), TokenKind::Arrow | TokenKind::Equals) {
                    self.bump();
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::Underscore) {
                        self.bump();
                        Some("_".into())
                    } else {
                        Some(self.expect_ident().0)
                    }
                } else {
                    None
                };
                sels.push(ImportSelector {
                    name,
                    rename,
                    span: nsp.merge(self.prev_span()),
                });
            }
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        sels
    }

    fn parse_ident_tree(&mut self) -> Tree {
        let (name, sp) = self.expect_ident();
        self.alloc(sp, TreeKind::Ident { name })
    }

    fn parse_path(&mut self) -> Tree {
        let mut t = {
            if matches!(self.kind(), TokenKind::This) {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::This { qual: None })
            } else {
                self.parse_ident_tree()
            }
        };
        loop {
            if !matches!(self.kind(), TokenKind::Dot) {
                break;
            }
            // don't skip nl before dot
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::This) {
                let sp = self.span();
                self.bump();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::This {
                        qual: t.name().map(|s| s.to_string()),
                    },
                );
            } else if matches!(self.kind(), TokenKind::Super) {
                let sp = self.span();
                self.bump();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::Super {
                        qual: t.name().map(|s| s.to_string()),
                        mix: None,
                    },
                );
            } else if matches!(self.kind(), TokenKind::TypeKw) {
                // `p.type` — stop so the type parser's singleton loop can
                // also attach `#` / `[T]` after we return. The Dot is already
                // consumed; build the singleton here.
                let sp = self.span();
                self.bump();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::SingletonTypeTree { ref_: Box::new(t) },
                );
                break;
            } else {
                let (name, sp) = self.expect_ident();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::Select {
                        qual: Box::new(t),
                        name,
                    },
                );
            }
        }
        t
    }

    // ------------------------------------------------------------------
    // Definitions
    // ------------------------------------------------------------------

    fn parse_modifiers(&mut self) -> Modifiers {
        let mut flags = Flags::EMPTY;
        let mut private_within = None;
        let mut annotations = Vec::new();
        loop {
            self.skip_nl();
            match self.kind() {
                TokenKind::Private => {
                    flags = flags.with(Flags::PRIVATE);
                    self.bump();
                    self.apply_access_qualifier(&mut flags, &mut private_within);
                }
                TokenKind::Protected => {
                    flags = flags.with(Flags::PROTECTED);
                    self.bump();
                    self.apply_access_qualifier(&mut flags, &mut private_within);
                }
                TokenKind::Abstract => {
                    flags = flags.with(Flags::ABSTRACT);
                    self.bump();
                }
                TokenKind::Final => {
                    flags = flags.with(Flags::FINAL);
                    self.bump();
                }
                TokenKind::Sealed => {
                    flags = flags.with(Flags::SEALED);
                    self.bump();
                }
                TokenKind::Implicit => {
                    flags = flags.with(Flags::IMPLICIT);
                    self.bump();
                }
                TokenKind::Lazy => {
                    flags = flags.with(Flags::LAZY);
                    self.bump();
                }
                TokenKind::Override => {
                    flags = flags.with(Flags::OVERRIDE);
                    self.bump();
                }
                TokenKind::Case => {
                    // `case class` / `case object` — don't consume if it's a match case.
                    // At def-start, peek next.
                    flags = flags.with(Flags::CASE);
                    self.bump();
                }
                TokenKind::At => {
                    let sp = self.span();
                    self.bump();
                    let annot = self.parse_simple_expr();
                    let path = annot.annotation_path();
                    if annotation_supported(&path) {
                        let simple = path.rsplit('.').next().unwrap_or(path.as_str());
                        if simple == "volatile" {
                            flags = flags.with(Flags::VOLATILE);
                        } else if simple == "transient" {
                            flags = flags.with(Flags::TRANSIENT);
                        } else if simple == "native" {
                            flags = flags.with(Flags::NATIVE);
                        }
                        annotations.push(annot);
                    } else {
                        let shown = if path.is_empty() {
                            "annotation".into()
                        } else {
                            format!("annotation {path}")
                        };
                        self.error_span(sp, format!("unimplemented syntax: {shown}"));
                    }
                }
                _ => break,
            }
        }
        Modifiers {
            flags,
            private_within,
            annotations,
        }
    }

    fn apply_access_qualifier(&mut self, flags: &mut Flags, private_within: &mut Option<String>) {
        if let Some(q) = self.parse_access_qualifier() {
            if q == "this" {
                *flags = flags.with(Flags::LOCAL);
            } else {
                *private_within = Some(q);
            }
        }
    }

    fn parse_access_qualifier(&mut self) -> Option<String> {
        if matches!(self.kind(), TokenKind::LBracket) {
            self.bump();
            self.skip_nl();
            let q = if matches!(self.kind(), TokenKind::This) {
                self.bump();
                "this".to_string()
            } else {
                self.expect_ident().0
            };
            self.expect("]", |k| matches!(k, TokenKind::RBracket));
            Some(q)
        } else {
            None
        }
    }

    fn parse_tmpl_or_def(&mut self) -> Tree {
        let mods = self.parse_modifiers();
        self.skip_nl();
        match self.kind() {
            TokenKind::Class => self.parse_class(mods, false),
            TokenKind::Trait => self.parse_class(mods, true),
            TokenKind::Object => self.parse_object_rest(mods, self.span(), false),
            TokenKind::Val | TokenKind::Var => self.parse_val_def(mods),
            TokenKind::Def => self.parse_def_def(mods),
            TokenKind::TypeKw => self.parse_type_def(mods),
            TokenKind::Import => self.parse_import(),
            TokenKind::Macro => self.unimplemented(self.span(), "macros"),
            TokenKind::Case => {
                // leftover `case` that is a match case at top level — error
                self.error_here("unexpected `case` (match cases belong inside `match { ... }`)");
                self.bump();
                self.empty(self.span())
            }
            _ => {
                if mods.flags.0 != 0 {
                    self.error_here("modifiers must be followed by a definition");
                }
                self.parse_expr()
            }
        }
    }

    fn parse_class(&mut self, mut mods: Modifiers, is_trait: bool) -> Tree {
        let lo = self.span();
        self.bump(); // class / trait
        if is_trait {
            mods.flags = mods.flags.with(Flags::TRAIT);
        }
        let (name, _) = self.expect_ident();
        self.skip_nl();
        let tparams = self.parse_type_param_clause();
        self.skip_nl();
        let ctor_mods = if is_trait {
            Modifiers::default()
        } else {
            // optional ctor access modifier
            self.parse_modifiers()
        };
        self.skip_nl();
        let vparamss = if is_trait {
            if matches!(self.kind(), TokenKind::LParen) {
                self.parse_param_clauses()
            } else {
                vec![]
            }
        } else {
            self.parse_param_clauses()
        };
        let impl_ = self.parse_template_opt(is_trait);
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::ClassDef {
                mods,
                name,
                tparams,
                ctor_mods,
                vparamss,
                impl_,
            },
        )
    }

    fn parse_object_rest(&mut self, mods: Modifiers, lo: Span, _pkg: bool) -> Tree {
        self.bump(); // object
        let (name, _) = self.expect_ident();
        let impl_ = self.parse_template_opt(false);
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::ModuleDef { mods, name, impl_ },
        )
    }

    /// `package object p { ... }` → `package p { object package { ... } }`.
    fn parse_package_object(&mut self, lo: Span) -> Tree {
        self.bump(); // object
        let (pkg_name, _) = self.expect_ident();
        let impl_ = self.parse_template_opt(false);
        let pid = self.alloc(lo, TreeKind::Ident { name: pkg_name });
        let module = self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::ModuleDef {
                mods: Modifiers::new(Flags::PACKAGE.with(Flags::MODULE)),
                name: "package".into(),
                impl_,
            },
        );
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::PackageDef {
                pid: Box::new(pid),
                stats: vec![module],
            },
        )
    }

    fn parse_type_param_clause(&mut self) -> Vec<Tree> {
        if !matches!(self.kind(), TokenKind::LBracket) {
            return vec![];
        }
        self.bump();
        let mut ts = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::RBracket) {
                self.bump();
                break;
            }
            ts.push(self.parse_type_param());
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Comma) {
                self.bump();
            } else if matches!(self.kind(), TokenKind::RBracket) {
                self.bump();
                break;
            } else {
                self.expect("]", |k| matches!(k, TokenKind::RBracket));
                break;
            }
        }
        ts
    }

    fn parse_type_param(&mut self) -> Tree {
        let lo = self.span();
        let mut mods = Modifiers::default();
        if matches!(self.kind(), TokenKind::Ident(s) if s == "+") {
            mods.flags = mods.flags.with(Flags::COVARIANT);
            self.bump();
        } else if matches!(self.kind(), TokenKind::Ident(s) if s == "-") {
            mods.flags = mods.flags.with(Flags::CONTRAVARIANT);
            self.bump();
        }
        let (name, _) = if matches!(self.kind(), TokenKind::Underscore) {
            let sp = self.span();
            self.bump();
            ("_".into(), sp)
        } else {
            self.expect_ident()
        };
        let inner_tparams = self.parse_type_param_clause();
        let mut lo_b = None;
        let mut hi_b = None;
        let mut views = Vec::new();
        let mut ctx_bounds = Vec::new();
        loop {
            self.skip_nl();
            match self.kind() {
                TokenKind::Subtype => {
                    self.bump();
                    hi_b = Some(Box::new(self.parse_type()));
                }
                TokenKind::Supertype => {
                    self.bump();
                    lo_b = Some(Box::new(self.parse_type()));
                }
                TokenKind::ViewBound => {
                    self.bump();
                    views.push(self.parse_type());
                }
                TokenKind::Colon => {
                    self.bump();
                    ctx_bounds.push(self.parse_type());
                }
                _ => break,
            }
        }
        let rhs = self.empty(self.span());
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::TypeDef {
                mods,
                name,
                tparams: inner_tparams,
                rhs: Box::new(rhs),
                lo: lo_b,
                hi: hi_b,
                views,
                ctx_bounds,
            },
        )
    }

    fn parse_param_clauses(&mut self) -> Vec<Vec<Tree>> {
        let mut clauses = Vec::new();
        loop {
            self.skip_nl();
            if !matches!(self.kind(), TokenKind::LParen) {
                break;
            }
            clauses.push(self.parse_param_clause());
        }
        clauses
    }

    fn parse_param_clause(&mut self) -> Vec<Tree> {
        self.bump(); // (
        let mut implicit = false;
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Implicit) {
            implicit = true;
            self.bump();
            self.skip_nl();
        }
        let mut params = Vec::new();
        if !matches!(self.kind(), TokenKind::RParen) {
            loop {
                params.push(self.parse_param(implicit));
                self.skip_nl();
                if matches!(self.kind(), TokenKind::Comma) {
                    self.bump();
                    self.skip_nl();
                } else {
                    break;
                }
            }
        }
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        params
    }

    fn parse_param(&mut self, implicit: bool) -> Tree {
        let lo = self.span();
        let mut mods = self.parse_modifiers();
        if implicit {
            mods.flags = mods.flags.with(Flags::IMPLICIT);
        }
        mods.flags = mods.flags.with(Flags::PARAM);
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Val) {
            mods.flags = mods.flags.with(Flags::ACCESSOR);
            self.bump();
        } else if matches!(self.kind(), TokenKind::Var) {
            mods.flags = mods.flags.with(Flags::MUTABLE);
            self.bump();
        }
        self.skip_nl();
        let (name, _) = if matches!(self.kind(), TokenKind::Underscore) {
            let sp = self.span();
            self.bump();
            ("_".into(), sp)
        } else {
            self.expect_ident()
        };
        self.skip_nl();
        let mut by_name = false;
        let mut tpt = if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Arrow) {
                by_name = true;
                self.bump();
                self.skip_nl();
            }
            self.parse_param_type()
        } else {
            self.empty(self.span())
        };
        if by_name {
            mods.flags = mods.flags.with(Flags::BYNAME);
        }
        self.skip_nl();
        // varargs `T*` → AppliedTypeTree `<repeated>[T]`
        if matches!(self.kind(), TokenKind::Ident(s) if s == "*") && !tpt.is_empty() {
            let sp = self.span();
            self.bump();
            let repeated = self.alloc(
                sp,
                TreeKind::Ident {
                    name: "<repeated>".into(),
                },
            );
            tpt = self.alloc(
                tpt.span.merge(sp),
                TreeKind::AppliedTypeTree {
                    tpt: Box::new(repeated),
                    args: vec![tpt],
                },
            );
        }
        let rhs = if matches!(self.kind(), TokenKind::Equals) {
            mods.flags = mods.flags.with(Flags::DEFAULTPARAM);
            self.bump();
            self.parse_expr()
        } else {
            self.empty(self.span())
        };
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::ValDef {
                mods,
                name,
                tpt: Box::new(tpt),
                rhs: Box::new(rhs),
            },
        )
    }

    fn parse_param_type(&mut self) -> Tree {
        self.parse_type()
    }

    fn parse_template_opt(&mut self, is_trait: bool) -> Template {
        self.skip_nl();
        let lo = self.span();
        let mut parents = Vec::new();
        let mut self_name = None;
        let mut self_tpt = None;
        let mut body = Vec::new();
        if matches!(self.kind(), TokenKind::Extends) {
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::LBrace) {
                let (sn, st, mut stats) = self.parse_template_body();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::With) {
                    // nsc EarlyDefs: `extends { val x = 1 } with T`
                    self.mark_early_defs(&mut stats);
                    body = stats;
                    self.bump();
                    self.skip_nl();
                    parents.push(self.parse_parent());
                    self.parse_with_parents(&mut parents);
                    if matches!(self.peek_non_nl(), TokenKind::LBrace) {
                        self.skip_nl();
                        let (sn2, st2, rest) = self.parse_template_body();
                        self_name = sn2;
                        self_tpt = st2;
                        body.extend(rest);
                    }
                    let _ = (sn, st);
                } else {
                    // `extends { ... }` without `with` is a regular template body.
                    self_name = sn;
                    self_tpt = st;
                    body = stats;
                }
            } else {
                parents.push(self.parse_parent());
                self.parse_with_parents(&mut parents);
                if matches!(self.peek_non_nl(), TokenKind::LBrace) {
                    self.skip_nl();
                    let (sn, st, rest) = self.parse_template_body();
                    self_name = sn;
                    self_tpt = st;
                    body = rest;
                }
            }
        } else if is_trait && !matches!(self.kind(), TokenKind::LBrace) {
            // trait T  (no body)
        } else if matches!(self.peek_non_nl(), TokenKind::LBrace) {
            self.skip_nl();
            let (sn, st, rest) = self.parse_template_body();
            self_name = sn;
            self_tpt = st;
            body = rest;
        }
        Template {
            parents,
            self_name,
            self_tpt,
            body,
            span: lo.merge(self.prev_span()),
        }
    }

    fn parse_with_parents(&mut self, parents: &mut Vec<Tree>) {
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::With) {
                self.bump();
                self.skip_nl();
                parents.push(self.parse_parent());
            } else {
                break;
            }
        }
    }

    /// nsc: only concrete field definitions are allowed in the early section.
    fn mark_early_defs(&mut self, stats: &mut [Tree]) {
        for s in stats {
            match &mut s.kind {
                TreeKind::ValDef { mods, rhs, .. }
                    if !rhs.is_empty() && !mods.flags.contains(Flags::LAZY) =>
                {
                    mods.flags = mods.flags.with(Flags::PRESUPER);
                }
                TreeKind::TypeDef { .. } => {}
                _ => {
                    self.error_span(
                        s.span,
                        "only concrete field definitions allowed in early object initialization section",
                    );
                }
            }
        }
    }

    fn parse_parent(&mut self) -> Tree {
        // AnnotType [ArgumentExprs]. Constructor argument lists stay on the
        // same line (`new Foo(1)`). A newline after the class name is a
        // statement separator (`val b = new Box` / `b.x = 3`), matching nsc.
        let mut out = self.parse_annot_type();
        // `class B extends A(1)(2)`: a parent takes as many argument lists as
        // the constructor has.
        while matches!(self.kind(), TokenKind::LParen) {
            let args = self.parse_arg_exprs();
            out = self.alloc(
                out.span.merge(self.prev_span()),
                TreeKind::Apply {
                    fun: Box::new(out),
                    args,
                },
            );
        }
        out
    }

    fn parse_template_body(&mut self) -> (Option<String>, Option<Box<Tree>>, Vec<Tree>) {
        self.bump(); // {
        self.skip_nl_semi();
        // self type: `self: T =>` or `this: T =>`
        let mut self_name = None;
        let mut self_tpt = None;
        if self.looks_like_self_type() {
            if matches!(self.kind(), TokenKind::This) {
                self.bump();
                self_name = Some("this".into());
            } else {
                self_name = Some(self.expect_ident().0);
            }
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Colon) {
                self.bump();
                // Infix/compound type only: `self: Foo =>` must not parse `=>` as `Function1`.
                self_tpt = Some(Box::new(self.parse_infix_type()));
            }
            self.skip_nl();
            self.expect("=>", |k| matches!(k, TokenKind::Arrow));
        }
        let body = self.parse_stats_until_rbrace();
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        (self_name, self_tpt, body)
    }

    fn looks_like_self_type(&self) -> bool {
        // ident/:this  :  type  =>
        let mut i = self.pos;
        let k = loop {
            if i >= self.tokens.len() {
                return false;
            }
            if matches!(self.tokens[i].kind, TokenKind::Newline) {
                i += 1;
                continue;
            }
            break &self.tokens[i].kind;
        };
        if !matches!(k, TokenKind::Ident(_) | TokenKind::This) {
            return false;
        }
        i += 1;
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        // `trait T { self => … }`: a bare name followed by `=>` is a self type
        // without an ascription.
        if i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Arrow) {
            return true;
        }
        if i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Colon) {
            // scan for => before ; or unbalanced
            let mut depth = 0;
            while i < self.tokens.len() {
                match self.tokens[i].kind {
                    TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
                    TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                        if depth == 0 {
                            return false;
                        }
                        depth -= 1;
                    }
                    TokenKind::Arrow if depth == 0 => return true,
                    TokenKind::Semi | TokenKind::Eof => return false,
                    _ => {}
                }
                i += 1;
            }
        }
        false
    }

    fn parse_stats_until_rbrace(&mut self) -> Vec<Tree> {
        let saved = std::mem::take(&mut self.placeholder_params);
        let mut stats = Vec::new();
        loop {
            self.skip_nl_semi();
            if self.at_eof() || matches!(self.kind(), TokenKind::RBrace) {
                break;
            }
            stats.extend(flatten_val_block(self.parse_block_stat()));
            if !self.accept_separator()
                && !matches!(self.kind(), TokenKind::RBrace | TokenKind::Eof)
            {
                if !is_def_start(self.kind()) && !matches!(self.kind(), TokenKind::Case) {
                    // might be ok — next token starts next stat
                }
            }
        }
        self.finish_no_escaping(saved);
        stats
    }

    fn parse_block_stat(&mut self) -> Tree {
        self.skip_nl();
        if is_mod_or_def_start(self.kind()) {
            return self.parse_tmpl_or_def();
        }
        self.parse_expr()
    }

    fn parse_val_def(&mut self, mut mods: Modifiers) -> Tree {
        let lo = self.span();
        let is_var = matches!(self.kind(), TokenKind::Var);
        self.bump();
        if is_var {
            mods.flags = mods.flags.with(Flags::MUTABLE);
        }
        self.skip_nl();
        // `val x: T = e` — do not use parse_pattern1, which would eat `: T` as a typed pattern.
        let pat = self.parse_pattern2();
        // `val Red, Blue = Value` — nsc PatDef with multiple patterns; each gets its own rhs.
        let mut extra_names: Vec<String> = Vec::new();
        if matches!(&pat.kind, TreeKind::Ident { .. } | TreeKind::Wildcard) {
            while matches!(self.kind(), TokenKind::Comma) {
                self.bump();
                self.skip_nl();
                extra_names.push(self.expect_ident().0);
            }
        }
        self.skip_nl();
        let tpt = if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.parse_type()
        } else {
            self.empty(self.span())
        };
        self.skip_nl();
        let rhs = if matches!(self.kind(), TokenKind::Equals) {
            self.bump();
            self.parse_expr()
        } else {
            if !mods.flags.contains(Flags::ABSTRACT) && !mods.flags.contains(Flags::PARAM) {
                // abstract val in trait is ok without rhs
            }
            self.empty(self.span())
        };
        let name = match &pat.kind {
            TreeKind::Ident { name } | TreeKind::Bind { name, .. } => name.clone(),
            TreeKind::Wildcard => "_".into(),
            _ => {
                // pattern val — keep a synthetic name and stash pattern in Bind
                "<pat>".into()
            }
        };
        let span = lo.merge(self.prev_span());
        // If it's a true pattern (not a simple ident), wrap.
        if matches!(&pat.kind, TreeKind::Ident { .. } | TreeKind::Wildcard) {
            let first = self.alloc(
                span,
                TreeKind::ValDef {
                    mods: mods.clone(),
                    name,
                    tpt: Box::new(tpt.clone()),
                    rhs: Box::new(rhs.clone()),
                },
            );
            if extra_names.is_empty() {
                first
            } else {
                let mut rest: Vec<Tree> = extra_names
                    .into_iter()
                    .map(|n| {
                        self.alloc(
                            span,
                            TreeKind::ValDef {
                                mods: mods.clone(),
                                name: n,
                                tpt: Box::new(tpt.clone()),
                                rhs: Box::new(rhs.clone()),
                            },
                        )
                    })
                    .collect();
                let expr = rest.pop().unwrap();
                let mut stats = vec![first];
                stats.append(&mut rest);
                self.alloc(
                    span,
                    TreeKind::Block {
                        stats,
                        expr: Box::new(expr),
                    },
                )
            }
        } else {
            // nsc desugars a pattern definition to a temporary plus one
            // selector per bound name:
            //   val (a, b) = e
            //     ~> val x$pat1 = e
            //        val a = x$pat1 match { case (a, b) => a }
            //        val b = x$pat1 match { case (a, b) => b }
            self.placeholder_id += 1;
            let tmp = format!("x$pat{}", self.placeholder_id);
            let tmp_def = self.alloc(
                span,
                TreeKind::ValDef {
                    mods: mods.clone(),
                    name: tmp.clone(),
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            );
            let mut names = Vec::new();
            pattern_bound_names(&pat, &mut names);
            let mut stats = vec![tmp_def];
            if names.is_empty() {
                // No bindings: still run the match so a mismatch is an error.
                let sel = self.alloc(span, TreeKind::Ident { name: tmp });
                let body = self.alloc(span, TreeKind::Literal { lit: Lit::Unit });
                let guard = self.empty(span);
                let m = self.alloc(
                    span,
                    TreeKind::Match {
                        selector: Box::new(sel),
                        cases: vec![CaseDef {
                            pat,
                            guard,
                            body,
                            span,
                        }],
                    },
                );
                stats.push(m);
            } else {
                for n in names {
                    let sel = self.alloc(span, TreeKind::Ident { name: tmp.clone() });
                    let body = self.alloc(span, TreeKind::Ident { name: n.clone() });
                    let guard = self.empty(span);
                    let m = self.alloc(
                        span,
                        TreeKind::Match {
                            selector: Box::new(sel),
                            cases: vec![CaseDef {
                                pat: pat.clone(),
                                guard,
                                body,
                                span,
                            }],
                        },
                    );
                    let empty_tpt = self.empty(span);
                    let def = self.alloc(
                        span,
                        TreeKind::ValDef {
                            mods: mods.clone(),
                            name: n,
                            tpt: Box::new(empty_tpt),
                            rhs: Box::new(m),
                        },
                    );
                    stats.push(def);
                }
            }
            let expr = stats.pop().unwrap();
            self.alloc(
                span,
                TreeKind::Block {
                    stats,
                    expr: Box::new(expr),
                },
            )
        }
    }

    fn parse_def_def(&mut self, mods: Modifiers) -> Tree {
        let lo = self.span();
        self.bump(); // def
        self.skip_nl();
        if matches!(self.kind(), TokenKind::This) {
            // auxiliary constructor
            self.bump();
            let vparamss = self.parse_param_clauses();
            self.skip_nl();
            self.expect("=", |k| matches!(k, TokenKind::Equals));
            let rhs = self.parse_expr();
            let tpt = self.empty(self.span());
            return self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::DefDef {
                    mods,
                    name: "<init>".into(),
                    tparams: vec![],
                    vparamss,
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            );
        }
        let (name, _) = self.expect_ident();
        self.skip_nl();
        let tparams = self.parse_type_param_clause();
        self.skip_nl();
        let vparamss = self.parse_param_clauses();
        self.skip_nl();
        let tpt = if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.parse_type()
        } else {
            self.empty(self.span())
        };
        self.skip_nl();
        let rhs = if matches!(self.kind(), TokenKind::Equals) {
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Macro) {
                return self.unimplemented(self.span(), "macros");
            }
            self.parse_expr()
        } else if matches!(self.peek_non_nl(), TokenKind::LBrace) {
            self.skip_nl();
            self.parse_block_expr()
        } else {
            self.empty(self.span())
        };
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::DefDef {
                mods,
                name,
                tparams,
                vparamss,
                tpt: Box::new(tpt),
                rhs: Box::new(rhs),
            },
        )
    }

    fn parse_type_def(&mut self, mods: Modifiers) -> Tree {
        let lo = self.span();
        self.bump();
        let (name, _) = self.expect_ident();
        let tparams = self.parse_type_param_clause();
        self.skip_nl();
        let mut lo_b = None;
        let mut hi_b = None;
        let mut rhs = self.empty(self.span());
        if matches!(self.kind(), TokenKind::Equals) {
            self.bump();
            rhs = self.parse_type();
        } else {
            if matches!(self.kind(), TokenKind::Supertype) {
                self.bump();
                lo_b = Some(Box::new(self.parse_type()));
            }
            if matches!(self.kind(), TokenKind::Subtype) {
                self.bump();
                hi_b = Some(Box::new(self.parse_type()));
            }
        }
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::TypeDef {
                mods,
                name,
                tparams,
                rhs: Box::new(rhs),
                lo: lo_b,
                hi: hi_b,
                views: vec![],
                ctx_bounds: vec![],
            },
        )
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn parse_type(&mut self) -> Tree {
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Arrow) {
            // by-name type in param already handled; here `=> T` as function 0
            let lo = self.span();
            self.bump();
            let rhs = self.parse_type();
            let tpt = self.alloc(
                lo,
                TreeKind::Ident {
                    name: "Function0".into(),
                },
            );
            let fn0 = self.alloc(
                lo.merge(rhs.span),
                TreeKind::AppliedTypeTree {
                    tpt: Box::new(tpt),
                    args: vec![rhs],
                },
            );
            return self.parse_existential_suffix(fn0);
        }
        let t = self.parse_infix_type();
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Arrow) {
            self.bump();
            let rhs = self.parse_type();
            let params = match &t.kind {
                TreeKind::AppliedTypeTree { tpt, args } if matches!(&tpt.kind, TreeKind::Ident { name } if name.starts_with("Tuple") || name == "<tuple>") => {
                    args.clone()
                }
                _ if is_unit_tuple(&t) => vec![],
                _ => vec![t.clone()],
            };
            // Represent A => B as AppliedTypeTree FunctionN
            let n = params.len();
            let mut args = params;
            args.push(rhs);
            let tpt = self.alloc(
                t.span,
                TreeKind::Ident {
                    name: format!("Function{n}"),
                },
            );
            let fn_ty = self.alloc(
                t.span.merge(self.prev_span()),
                TreeKind::AppliedTypeTree {
                    tpt: Box::new(tpt),
                    args,
                },
            );
            return self.parse_existential_suffix(fn_ty);
        }
        self.parse_existential_suffix(t)
    }

    /// `T forSome { type X; ... }`. Unsupported clauses stay in the tree and
    /// are diagnosed (not dropped).
    fn parse_existential_suffix(&mut self, t: Tree) -> Tree {
        self.skip_nl();
        if !matches!(self.kind(), TokenKind::ForSome) {
            return t;
        }
        let kw = self.span();
        self.bump();
        self.skip_nl();
        self.expect("{", |k| matches!(k, TokenKind::LBrace));
        let mut clauses = Vec::new();
        loop {
            self.skip_nl_semi();
            match self.kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                TokenKind::TypeKw => clauses.push(self.parse_type_def(Modifiers::default())),
                TokenKind::Val | TokenKind::Var => {
                    clauses.push(self.parse_val_def(Modifiers::default()));
                }
                TokenKind::Def => {
                    let sp = self.span();
                    clauses
                        .push(self.unimplemented(sp, "method existentials (`forSome { def … }`)"));
                    self.skip_to_existential_sep();
                }
                _ => {
                    let sp = self.span();
                    clauses.push(self.unimplemented(
                        sp,
                        "existential clause (only unbounded `type X` is supported)",
                    ));
                    self.skip_to_existential_sep();
                }
            }
        }
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        let _ = kw;
        self.alloc(
            t.span.merge(self.prev_span()),
            TreeKind::ExistentialTypeTree {
                tpt: Box::new(t),
                clauses,
            },
        )
    }

    fn skip_to_existential_sep(&mut self) {
        if !matches!(
            self.kind(),
            TokenKind::RBrace | TokenKind::Eof | TokenKind::Semi | TokenKind::Newline
        ) {
            self.bump();
        }
        loop {
            match self.kind() {
                TokenKind::RBrace | TokenKind::Eof | TokenKind::Semi | TokenKind::Newline => {
                    return;
                }
                TokenKind::TypeKw | TokenKind::Val | TokenKind::Var | TokenKind::Def => return,
                TokenKind::LBrace => self.skip_balanced_brace(),
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// The type in a typed pattern. Same as `parse_infix_type` except that `|`
    /// ends it, because alternation binds looser than the ascription.
    fn parse_pattern_type(&mut self) -> Tree {
        let saved_no_bar = std::mem::replace(&mut self.no_bar_infix_type, true);
        let t = self.parse_infix_type();
        self.no_bar_infix_type = saved_no_bar;
        t
    }

    fn parse_infix_type(&mut self) -> Tree {
        let mut t = self.parse_compound_type();
        loop {
            // infix type: T ident T  (ident not nl-separated in a way that starts a val)
            if !matches!(self.kind(), TokenKind::Ident(_)) {
                break;
            }
            // Don't treat following ident as infix if it's a def start on a new "statement"
            // In types, `T Either U` is infix. If next is ident and then a type, take it.
            if self.no_bar_infix_type && matches!(self.kind(), TokenKind::Ident(n) if n == "|") {
                break;
            }
            let saved = self.pos;
            let (name, nsp) = self.expect_ident();
            self.skip_nl();
            // postfix repeated `T*` — nsc does not parse this as infix `T * <error>`
            if name == "*" && !self.at_type_start() {
                let repeated = self.alloc(
                    nsp,
                    TreeKind::Ident {
                        name: "<repeated>".into(),
                    },
                );
                t = self.alloc(
                    t.span.merge(nsp),
                    TreeKind::AppliedTypeTree {
                        tpt: Box::new(repeated),
                        args: vec![t],
                    },
                );
                continue;
            }
            if is_operator_name(&name) || self.looks_like_type_start() {
                let rhs = self.parse_compound_type();
                let tpt = self.alloc(nsp, TreeKind::Ident { name });
                t = self.alloc(
                    t.span.merge(rhs.span),
                    TreeKind::AppliedTypeTree {
                        tpt: Box::new(tpt),
                        args: vec![t, rhs],
                    },
                );
            } else {
                self.pos = saved;
                break;
            }
        }
        t
    }

    fn at_type_start(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Ident(_)
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::This
                | TokenKind::Super
                | TokenKind::Underscore
                | TokenKind::IntLit(_)
                | TokenKind::LongLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::CharLit(_)
                | TokenKind::StringLit(_)
                | TokenKind::SymbolLit(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
        )
    }

    fn looks_like_prefix_start(&self) -> bool {
        match self.kind() {
            TokenKind::Ident(s) if matches!(s.as_str(), "+" | "-" | "!" | "~") => true,
            TokenKind::New
            | TokenKind::LBrace
            | TokenKind::LParen
            | TokenKind::This
            | TokenKind::Super
            | TokenKind::Underscore
            | TokenKind::Ident(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::IntLit(_)
            | TokenKind::LongLit(_)
            | TokenKind::FloatLit(_)
            | TokenKind::DoubleLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::StringLit(_)
            | TokenKind::SymbolLit(_)
            | TokenKind::InterpStart { .. } => true,
            _ => false,
        }
    }

    fn looks_like_type_start(&self) -> bool {
        matches!(
            self.peek_non_nl(),
            TokenKind::Ident(_)
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::This
                | TokenKind::Super
                | TokenKind::Underscore
                | TokenKind::IntLit(_)
                | TokenKind::LongLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::CharLit(_)
                | TokenKind::StringLit(_)
                | TokenKind::SymbolLit(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
        )
    }

    fn parse_compound_type(&mut self) -> Tree {
        self.skip_nl();
        if matches!(self.kind(), TokenKind::LBrace) {
            let lo = self.span();
            let refinements = self.parse_refinement();
            return self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::CompoundTypeTree {
                    parents: vec![],
                    refinements,
                },
            );
        }
        let t = self.parse_annot_type();
        let mut parents = vec![t.clone()];
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::With) {
                self.bump();
                parents.push(self.parse_annot_type());
            } else {
                break;
            }
        }
        self.skip_nl();
        let refinements = if matches!(self.kind(), TokenKind::LBrace) {
            self.parse_refinement()
        } else {
            vec![]
        };
        if parents.len() == 1 && refinements.is_empty() {
            t
        } else {
            self.alloc(
                parents[0].span.merge(self.prev_span()),
                TreeKind::CompoundTypeTree {
                    parents,
                    refinements,
                },
            )
        }
    }

    fn parse_refinement(&mut self) -> Vec<Tree> {
        self.bump(); // {
        let mut decls = Vec::new();
        loop {
            self.skip_nl_semi();
            match self.kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                TokenKind::TypeKw => decls.push(self.parse_type_def(Modifiers::default())),
                TokenKind::Val => {
                    let t = self.parse_val_def(Modifiers::default());
                    if refinement_has_impl(&t) {
                        self.error_span(t.span, "illegal implementation in refinement");
                        decls.push(self.alloc(
                            t.span,
                            TreeKind::Unimplemented {
                                what: "illegal implementation in refinement".into(),
                            },
                        ));
                    } else {
                        decls.push(t);
                    }
                }
                TokenKind::Var => {
                    let t = self.parse_val_def(Modifiers::default());
                    if refinement_has_impl(&t) {
                        self.error_span(t.span, "illegal implementation in refinement");
                        decls.push(self.alloc(
                            t.span,
                            TreeKind::Unimplemented {
                                what: "illegal implementation in refinement".into(),
                            },
                        ));
                    } else {
                        decls.push(t);
                    }
                }
                TokenKind::Def => {
                    let t = self.parse_def_def(Modifiers::default());
                    if refinement_has_impl(&t) {
                        self.error_span(t.span, "illegal implementation in refinement");
                        decls.push(self.alloc(
                            t.span,
                            TreeKind::Unimplemented {
                                what: "illegal implementation in refinement".into(),
                            },
                        ));
                    } else {
                        decls.push(t);
                    }
                }
                _ => {
                    let sp = self.span();
                    self.error_span(sp, "unimplemented: structural update");
                    decls.push(self.alloc(
                        sp,
                        TreeKind::Unimplemented {
                            what: "structural update".into(),
                        },
                    ));
                    self.skip_to_existential_sep();
                }
            }
        }
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        decls
    }

    fn skip_balanced_brace(&mut self) {
        if !matches!(self.kind(), TokenKind::LBrace) {
            return;
        }
        let mut d = 0;
        loop {
            match self.kind() {
                TokenKind::LBrace => d += 1,
                TokenKind::RBrace => {
                    d -= 1;
                    self.bump();
                    if d == 0 {
                        return;
                    }
                    continue;
                }
                TokenKind::Eof => return,
                _ => {}
            }
            self.bump();
        }
    }

    /// `x: @switch` — annotation ascription with no underlying type tree.
    fn parse_annot_ascription(&mut self) -> Tree {
        let mut t = self.empty(self.span());
        while matches!(self.kind(), TokenKind::At) {
            self.bump();
            let annot = self.parse_simple_expr();
            t = self.alloc(
                t.span.merge(self.prev_span()),
                TreeKind::AnnotatedTypeTree {
                    tpt: Box::new(t),
                    annot: Box::new(annot),
                },
            );
        }
        t
    }

    fn parse_annot_type(&mut self) -> Tree {
        let mut t = self.parse_simple_type();
        while matches!(self.kind(), TokenKind::At) {
            self.bump();
            let annot = self.parse_simple_expr();
            t = self.alloc(
                t.span.merge(self.prev_span()),
                TreeKind::AnnotatedTypeTree {
                    tpt: Box::new(t),
                    annot: Box::new(annot),
                },
            );
        }
        t
    }

    /// SIP-23 constant types in type position: `val x: 1 = 1`.
    fn parse_constant_type_lit(&mut self) -> Option<Tree> {
        let sp = self.span();
        let lit = match self.kind().clone() {
            TokenKind::IntLit(n) => Lit::Int(n),
            TokenKind::LongLit(n) => Lit::Long(n),
            TokenKind::FloatLit(n) => Lit::Float(n),
            TokenKind::DoubleLit(n) => Lit::Double(n),
            TokenKind::CharLit(c) => Lit::Char(c),
            TokenKind::StringLit(s) => Lit::String(s),
            TokenKind::SymbolLit(s) => Lit::Symbol(s),
            TokenKind::True => Lit::Boolean(true),
            TokenKind::False => Lit::Boolean(false),
            TokenKind::Null => Lit::Null,
            _ => return None,
        };
        self.bump();
        Some(self.alloc(sp, TreeKind::Literal { lit }))
    }

    fn parse_simple_type(&mut self) -> Tree {
        self.skip_nl();
        if let Some(lit) = self.parse_constant_type_lit() {
            return lit;
        }
        let mut t = match self.kind().clone() {
            TokenKind::LParen => {
                self.bump();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::RParen) {
                    let sp = self.span();
                    self.bump();
                    self.alloc(
                        sp,
                        TreeKind::Ident {
                            name: "Unit".into(),
                        },
                    )
                } else {
                    let mut ts = vec![self.parse_type()];
                    while matches!(self.kind(), TokenKind::Comma) {
                        self.bump();
                        ts.push(self.parse_type());
                    }
                    self.expect(")", |k| matches!(k, TokenKind::RParen));
                    if ts.len() == 1 {
                        ts.pop().unwrap()
                    } else {
                        let span = ts[0].span.merge(self.prev_span());
                        let tpt = self.alloc(
                            span,
                            TreeKind::Ident {
                                name: format!("Tuple{}", ts.len()),
                            },
                        );
                        self.alloc(
                            span,
                            TreeKind::AppliedTypeTree {
                                tpt: Box::new(tpt),
                                args: ts,
                            },
                        )
                    }
                }
            }
            TokenKind::Underscore => {
                let sp = self.span();
                self.bump();
                // wildcard type `_` / `_ <: T`
                let mut hi = None;
                let mut lo = None;
                if matches!(self.kind(), TokenKind::Subtype) {
                    self.bump();
                    hi = Some(Box::new(self.parse_type()));
                }
                if matches!(self.kind(), TokenKind::Supertype) {
                    self.bump();
                    lo = Some(Box::new(self.parse_type()));
                }
                let rhs = self.empty(sp);
                self.alloc(
                    sp.merge(self.prev_span()),
                    TreeKind::TypeDef {
                        mods: Modifiers::default(),
                        name: "_".into(),
                        tparams: vec![],
                        rhs: Box::new(rhs),
                        lo,
                        hi,
                        views: vec![],
                        ctx_bounds: vec![],
                    },
                )
            }
            TokenKind::This => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::This { qual: None })
            }
            _ => self.parse_path(),
        };
        // nsc SimpleType: dots belong to StableId *before* TypeArgs. After
        // `C[T]`, `.enqueue` is term selection (`new C[T].enqueue`), not a
        // type path. Type projection after args is `#`, not `.`.
        let mut seen_type_args = false;
        loop {
            if matches!(self.kind(), TokenKind::Dot) {
                if seen_type_args {
                    break;
                }
                self.bump();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::TypeKw) {
                    let sp = self.span();
                    self.bump();
                    t = self.alloc(
                        t.span.merge(sp),
                        TreeKind::SingletonTypeTree { ref_: Box::new(t) },
                    );
                    continue;
                }
                let (name, sp) = self.expect_ident();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::SelectFromTypeTree {
                        qual: Box::new(t),
                        name,
                        hash: false,
                    },
                );
                continue;
            }
            if matches!(self.kind(), TokenKind::Hash) {
                self.bump();
                let (name, sp) = self.expect_ident();
                t = self.alloc(
                    t.span.merge(sp),
                    TreeKind::SelectFromTypeTree {
                        qual: Box::new(t),
                        name,
                        hash: true,
                    },
                );
                continue;
            }
            if matches!(self.kind(), TokenKind::LBracket) {
                let args = self.parse_type_args();
                t = self.alloc(
                    t.span.merge(self.prev_span()),
                    TreeKind::AppliedTypeTree {
                        tpt: Box::new(t),
                        args,
                    },
                );
                seen_type_args = true;
                continue;
            }
            break;
        }
        t
    }

    fn parse_type_args(&mut self) -> Vec<Tree> {
        self.bump(); // [
        let mut args = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::RBracket) {
                self.bump();
                break;
            }
            args.push(self.parse_type());
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Comma) {
                self.bump();
            } else {
                self.expect("]", |k| matches!(k, TokenKind::RBracket));
                break;
            }
        }
        args
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn parse_expr(&mut self) -> Tree {
        self.with_placeholders(|p| p.parse_expr1())
    }

    fn parse_expr1(&mut self) -> Tree {
        self.skip_nl();
        match self.kind() {
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::Try => self.parse_try(),
            TokenKind::For => self.parse_for(),
            TokenKind::Throw => {
                let lo = self.span();
                self.bump();
                let e = self.parse_expr();
                self.alloc(lo.merge(e.span), TreeKind::Throw { expr: Box::new(e) })
            }
            TokenKind::Return => {
                let lo = self.span();
                self.bump();
                let expr = if self.kind().can_start_stat()
                    && !matches!(
                        self.kind(),
                        TokenKind::Newline | TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
                    )
                    && !matches!(
                        self.kind(),
                        TokenKind::Else | TokenKind::Catch | TokenKind::Finally
                    ) {
                    self.parse_expr()
                } else {
                    self.alloc(lo, TreeKind::Literal { lit: Lit::Unit })
                };
                self.alloc(
                    lo.merge(expr.span),
                    TreeKind::Return {
                        expr: Box::new(expr),
                    },
                )
            }
            TokenKind::Implicit => {
                // implicit lambda: implicit x => e
                let lo = self.span();
                self.bump();
                let params = vec![self.parse_param(true)];
                self.expect("=>", |k| matches!(k, TokenKind::Arrow));
                let body = self.parse_expr();
                self.alloc(
                    lo.merge(body.span),
                    TreeKind::Function {
                        vparams: params,
                        body: Box::new(body),
                    },
                )
            }
            _ => {
                let t = self.parse_postfix_expr();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::Equals) {
                    self.bump();
                    let rhs = self.parse_expr();
                    return self.alloc(
                        t.span.merge(rhs.span),
                        TreeKind::Assign {
                            lhs: Box::new(t),
                            rhs: Box::new(rhs),
                        },
                    );
                }
                if matches!(self.kind(), TokenKind::Arrow) {
                    self.bump();
                    let body = self.parse_expr();
                    let vparams = self.convert_to_params(t.clone());
                    return self.alloc(
                        t.span.merge(body.span),
                        TreeKind::Function {
                            vparams,
                            body: Box::new(body),
                        },
                    );
                }
                if matches!(self.kind(), TokenKind::Match) {
                    return self.parse_match_rest(t);
                }
                if matches!(self.kind(), TokenKind::Colon) {
                    self.bump();
                    // nsc Ascription ::= COLON InfixType | COLON Annotation {Annotation}
                    // so `(n: @switch) match` / `n: @switch match` typecheck.
                    let tpt = if matches!(self.kind(), TokenKind::At) {
                        self.parse_annot_ascription()
                    } else {
                        self.parse_type()
                    };
                    if is_placeholder_wildcard(&t, &self.placeholder_params) {
                        if let Some(p) = self.placeholder_params.last_mut() {
                            if let TreeKind::ValDef { tpt: pt, .. } = &mut p.kind {
                                *pt = Box::new(tpt.clone());
                            }
                        }
                    }
                    let typed = self.alloc(
                        t.span.merge(tpt.span),
                        TreeKind::Typed {
                            expr: Box::new(t),
                            tpt: Box::new(tpt),
                        },
                    );
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::Match) {
                        return self.parse_match_rest(typed);
                    }
                    return typed;
                }
                t
            }
        }
    }

    fn parse_if(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        self.skip_nl();
        self.expect("(", |k| matches!(k, TokenKind::LParen));
        let cond = self.parse_expr();
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        self.skip_nl();
        let thenp = self.parse_expr();
        // optional else; newline before else is not a separator
        let saved = self.pos;
        self.skip_nl();
        let elsep = if matches!(self.kind(), TokenKind::Else) {
            self.bump();
            self.parse_expr()
        } else {
            self.pos = saved;
            self.alloc(self.span(), TreeKind::Literal { lit: Lit::Unit })
        };
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::If {
                cond: Box::new(cond),
                thenp: Box::new(thenp),
                elsep: Box::new(elsep),
            },
        )
    }

    fn parse_while(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        self.expect("(", |k| matches!(k, TokenKind::LParen));
        let cond = self.parse_expr();
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        self.skip_nl();
        let body = self.parse_expr();
        self.alloc(
            lo.merge(body.span),
            TreeKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
        )
    }

    fn parse_do_while(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        let body = self.parse_expr();
        self.skip_nl();
        self.expect("while", |k| matches!(k, TokenKind::While));
        self.expect("(", |k| matches!(k, TokenKind::LParen));
        let cond = self.parse_expr();
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::DoWhile {
                body: Box::new(body),
                cond: Box::new(cond),
            },
        )
    }

    fn parse_try(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        let block = self.parse_expr();
        self.skip_nl();
        let mut catches = Vec::new();
        if matches!(self.kind(), TokenKind::Catch) {
            self.bump();
            self.skip_nl();
            // catch { cases } or catch expr (partial fn)
            if matches!(self.kind(), TokenKind::LBrace) {
                self.bump();
                catches = self.parse_cases();
                self.expect("}", |k| matches!(k, TokenKind::RBrace));
            } else {
                let _ = self.parse_expr();
                self.error_span(
                    lo,
                    "unimplemented syntax: `catch` of a non-block expression",
                );
            }
        }
        self.skip_nl();
        let finalizer = if matches!(self.kind(), TokenKind::Finally) {
            self.bump();
            self.parse_expr()
        } else {
            self.empty(self.span())
        };
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::Try {
                block: Box::new(block),
                catches,
                finalizer: Box::new(finalizer),
            },
        )
    }

    fn parse_for(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        self.skip_nl();
        let parens = if matches!(self.kind(), TokenKind::LParen) {
            self.bump();
            true
        } else if matches!(self.kind(), TokenKind::LBrace) {
            self.bump();
            false
        } else {
            self.error_here("expected `(` or `{` after `for`");
            true
        };
        let enums = self.parse_enumerators();
        if parens {
            self.expect(")", |k| matches!(k, TokenKind::RParen));
        } else {
            self.expect("}", |k| matches!(k, TokenKind::RBrace));
        }
        self.skip_nl();
        let is_yield = matches!(self.kind(), TokenKind::Yield);
        if is_yield {
            self.bump();
        }
        let body = self.parse_expr();
        desugar_for(self, lo, enums, body, is_yield)
    }

    fn parse_enumerators(&mut self) -> Vec<Enumerator> {
        let mut enums: Vec<Enumerator> = Vec::new();
        loop {
            self.skip_nl_semi();
            if matches!(
                self.kind(),
                TokenKind::RParen | TokenKind::RBrace | TokenKind::Yield | TokenKind::Eof
            ) {
                break;
            }
            if matches!(self.kind(), TokenKind::If) {
                self.bump();
                let g = self.parse_postfix_expr();
                if let Some(last) = enums.last_mut() {
                    last.guard = Some(g);
                } else {
                    self.error_here("guard without a generator");
                }
                self.accept_separator();
                continue;
            }
            let pat = self.parse_pattern1();
            self.skip_nl();
            let is_val = if matches!(self.kind(), TokenKind::LeftArrow) {
                self.bump();
                false
            } else if matches!(self.kind(), TokenKind::Equals) {
                self.bump();
                true
            } else {
                self.error_here("expected `<-` or `=` in enumerator");
                false
            };
            let rhs = self.parse_expr();
            let mut guard = None;
            self.skip_nl();
            if matches!(self.kind(), TokenKind::If) {
                self.bump();
                guard = Some(self.parse_postfix_expr());
            }
            enums.push(Enumerator {
                pat,
                rhs,
                is_val,
                guard,
            });
            self.accept_separator();
            if matches!(self.kind(), TokenKind::RParen | TokenKind::RBrace) {
                break;
            }
        }
        enums
    }

    fn parse_match_rest(&mut self, selector: Tree) -> Tree {
        self.bump(); // match
        self.skip_nl();
        self.expect("{", |k| matches!(k, TokenKind::LBrace));
        let cases = self.parse_cases();
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        self.alloc(
            selector.span.merge(self.prev_span()),
            TreeKind::Match {
                selector: Box::new(selector),
                cases,
            },
        )
    }

    fn parse_cases(&mut self) -> Vec<CaseDef> {
        let mut cases = Vec::new();
        loop {
            self.skip_nl_semi();
            if !matches!(self.kind(), TokenKind::Case) {
                break;
            }
            cases.push(self.parse_case());
        }
        cases
    }

    fn parse_case(&mut self) -> CaseDef {
        let lo = self.span();
        self.bump(); // case
        let pat = self.parse_pattern();
        self.skip_nl();
        let guard = if matches!(self.kind(), TokenKind::If) {
            self.bump();
            self.parse_postfix_expr()
        } else {
            self.empty(self.span())
        };
        self.expect("=>", |k| matches!(k, TokenKind::Arrow));
        let body = self.parse_case_body();
        CaseDef {
            span: lo.merge(body.span),
            pat,
            guard,
            body,
        }
    }

    fn parse_case_body(&mut self) -> Tree {
        // stats until next case or }
        let lo = self.span();
        let mut stats = Vec::new();
        loop {
            self.skip_nl();
            if matches!(
                self.kind(),
                TokenKind::Case | TokenKind::RBrace | TokenKind::Eof
            ) {
                break;
            }
            if matches!(self.kind(), TokenKind::Semi) {
                self.bump();
                continue;
            }
            stats.push(self.parse_block_stat());
            self.accept_separator();
        }
        block_from_stats(self, lo, stats)
    }

    fn parse_postfix_expr(&mut self) -> Tree {
        let mut t = self.parse_infix_expr(0);
        // nsc PostfixExpr ::= InfixExpr [id] on the same line.
        if matches!(self.kind(), TokenKind::Ident(_)) {
            if let Some(name) = self.ident_text() {
                let sp = self.span();
                self.bump();
                let mut sel = self.alloc(
                    t.span.merge(sp),
                    TreeKind::Select {
                        qual: Box::new(t),
                        name,
                    },
                );
                sel.postfix = true;
                t = sel;
            }
        }
        // Eta-expansion `foo _` (nsc: Typed(foo, Function([], EmptyTree))).
        if matches!(self.kind(), TokenKind::Underscore) {
            let sp = self.span();
            self.bump();
            let empty = self.empty(sp);
            let fn_tpt = self.alloc(
                sp,
                TreeKind::Function {
                    vparams: vec![],
                    body: Box::new(empty),
                },
            );
            t = self.alloc(
                t.span.merge(sp),
                TreeKind::Typed {
                    expr: Box::new(t),
                    tpt: Box::new(fn_tpt),
                },
            );
        }
        t
    }

    fn parse_infix_expr(&mut self, min_prec: i32) -> Tree {
        let mut left = self.parse_prefix_expr();
        loop {
            // newline before operator continues the infix expr (Scala)
            let saved = self.pos;
            if matches!(self.kind(), TokenKind::Newline) {
                self.skip_nl();
            }
            let Some(op) = self.ident_text() else {
                self.pos = saved;
                break;
            };
            let prec = op_precedence(&op);
            if prec < min_prec {
                self.pos = saved;
                break;
            }
            // Don't treat letter-idents as infix if they could start a new statement
            // after a newline. `foo\nbar` is two stats; `foo\n+ bar` is infix.
            if saved != self.pos && !is_operator_name(&op) {
                self.pos = saved;
                break;
            }
            // If there is no right-hand prefix expr, this ident is postfix not infix
            // (`xs toList`, `42 abs`).
            let after_ident = self.pos;
            self.bump();
            if is_operator_name(&op) && matches!(self.kind(), TokenKind::Newline) {
                self.skip_nl();
            }
            if !self.looks_like_prefix_start() {
                self.pos = saved;
                break;
            }
            let op_span = self.tokens[after_ident].span;
            let right_assoc = op.ends_with(':');
            let next_min = if right_assoc { prec } else { prec + 1 };
            let right = self.parse_infix_expr(next_min);
            // Operators ending in `:` are right-associative and the receiver is
            // the right-hand operand (`1 :: Nil` → `Nil.::(1)`), matching nsc.
            left = if right_assoc {
                let sel = self.alloc(
                    right.span.merge(op_span),
                    TreeKind::Select {
                        qual: Box::new(right),
                        name: op,
                    },
                );
                self.alloc(
                    left.span.merge(sel.span),
                    TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![left],
                    },
                )
            } else {
                let sel = self.alloc(
                    left.span.merge(op_span),
                    TreeKind::Select {
                        qual: Box::new(left),
                        name: op,
                    },
                );
                self.alloc(
                    sel.span.merge(right.span),
                    TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![right],
                    },
                )
            };
        }
        left
    }

    fn parse_prefix_expr(&mut self) -> Tree {
        self.skip_nl();
        if let Some(name) = self.ident_text() {
            if matches!(name.as_str(), "+" | "-" | "!" | "~") {
                let sp = self.span();
                self.bump();
                let arg = self.parse_prefix_expr();
                let sel = self.alloc(
                    sp,
                    TreeKind::Select {
                        qual: Box::new(arg.clone()),
                        name: format!("unary_{name}"),
                    },
                );
                return self.alloc(
                    sp.merge(arg.span),
                    TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![],
                    },
                );
            }
        }
        self.parse_simple_expr()
    }

    fn parse_simple_expr(&mut self) -> Tree {
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Ident(s) if s == "<") {
            let t = self.parse_xml_literal();
            return self.parse_simple_expr_rest(t);
        }
        let mut t = match self.kind().clone() {
            TokenKind::New => self.parse_new(),
            TokenKind::LBrace => self.parse_block_expr(),
            TokenKind::LParen => self.parse_paren_expr(),
            TokenKind::This => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::This { qual: None })
            }
            TokenKind::Super => {
                let sp = self.span();
                self.bump();
                let mix = if matches!(self.kind(), TokenKind::LBracket) {
                    self.bump();
                    let n = self.expect_ident().0;
                    self.expect("]", |k| matches!(k, TokenKind::RBracket));
                    Some(n)
                } else {
                    None
                };
                self.alloc(
                    sp.merge(self.prev_span()),
                    TreeKind::Super { qual: None, mix },
                )
            }
            TokenKind::Underscore => self.fresh_placeholder(),
            // Lexer emits Ident("_") before `:` / `*` (`case _: T`, `_*`).
            // In expr position that token is nsc placeholder `_`, e.g. `(_: Int) + 1`.
            TokenKind::Ident(s) if s == "_" => self.fresh_placeholder(),
            TokenKind::Ident(_) => self.parse_ident_tree(),
            TokenKind::True => {
                let sp = self.span();
                self.bump();
                self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::Boolean(true),
                    },
                )
            }
            TokenKind::False => {
                let sp = self.span();
                self.bump();
                self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::Boolean(false),
                    },
                )
            }
            TokenKind::Null => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Literal { lit: Lit::Null })
            }
            TokenKind::IntLit(n) => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Literal { lit: Lit::Int(n) })
            }
            TokenKind::LongLit(n) => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Literal { lit: Lit::Long(n) })
            }
            TokenKind::DoubleLit(n) => {
                let sp = self.span();
                self.bump();
                self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::Double(n),
                    },
                )
            }
            TokenKind::FloatLit(n) => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Literal { lit: Lit::Float(n) })
            }
            TokenKind::CharLit(c) => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Literal { lit: Lit::Char(c) })
            }
            TokenKind::StringLit(s) => {
                let sp = self.span();
                self.bump();
                self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::String(s),
                    },
                )
            }
            TokenKind::SymbolLit(s) => {
                let sp = self.span();
                self.bump();
                self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::Symbol(s),
                    },
                )
            }
            TokenKind::InterpStart { prefix, .. } => self.parse_interpolated(prefix),
            other => {
                let sp = self.span();
                self.error_here(format!("expected expression, found {}", token_name(&other)));
                self.bump();
                self.empty(sp)
            }
        };
        t = self.parse_simple_expr_rest(t);
        t
    }

    fn parse_simple_expr_rest(&mut self, mut t: Tree) -> Tree {
        loop {
            match self.kind() {
                TokenKind::Dot => {
                    self.bump();
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::This) {
                        let sp = self.span();
                        self.bump();
                        t = self.alloc(
                            t.span.merge(sp),
                            TreeKind::This {
                                qual: t.name().map(|s| s.to_string()),
                            },
                        );
                    } else if matches!(self.kind(), TokenKind::Super) {
                        self.bump();
                        let mix = if matches!(self.kind(), TokenKind::LBracket) {
                            self.bump();
                            let n = self.expect_ident().0;
                            self.expect("]", |k| matches!(k, TokenKind::RBracket));
                            Some(n)
                        } else {
                            None
                        };
                        t = self.alloc(
                            t.span.merge(self.prev_span()),
                            TreeKind::Super {
                                qual: t.name().map(|s| s.to_string()),
                                mix,
                            },
                        );
                    } else if matches!(self.kind(), TokenKind::Underscore) {
                        let sp = self.span();
                        self.bump();
                        t = self.alloc(
                            t.span.merge(sp),
                            TreeKind::Select {
                                qual: Box::new(t),
                                name: "_".into(),
                            },
                        );
                    } else {
                        let (name, sp) = self.expect_ident();
                        t = self.alloc(
                            t.span.merge(sp),
                            TreeKind::Select {
                                qual: Box::new(t),
                                name,
                            },
                        );
                    }
                }
                TokenKind::LBracket => {
                    let args = self.parse_type_args();
                    t = self.alloc(
                        t.span.merge(self.prev_span()),
                        TreeKind::TypeApply {
                            fun: Box::new(t),
                            args,
                        },
                    );
                }
                TokenKind::LParen => {
                    let args = self.parse_arg_exprs();
                    t = self.alloc(
                        t.span.merge(self.prev_span()),
                        TreeKind::Apply {
                            fun: Box::new(t),
                            args,
                        },
                    );
                }
                TokenKind::LBrace => {
                    // block argument: foo { ... }  (same line or after nl that isn't a semi)
                    let blk = self.parse_block_expr();
                    t = self.alloc(
                        t.span.merge(blk.span),
                        TreeKind::Apply {
                            fun: Box::new(t),
                            args: vec![blk],
                        },
                    );
                }
                TokenKind::Newline => {
                    // `foo \n {` is application; `foo \n +` handled in infix;
                    // `foo \n bar` is two statements — don't consume.
                    let saved = self.pos;
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::LBrace) {
                        let blk = self.parse_block_expr();
                        t = self.alloc(
                            t.span.merge(blk.span),
                            TreeKind::Apply {
                                fun: Box::new(t),
                                args: vec![blk],
                            },
                        );
                    } else {
                        self.pos = saved;
                        break;
                    }
                }
                _ => break,
            }
        }
        t
    }

    fn parse_arg_exprs(&mut self) -> Vec<Tree> {
        self.bump(); // (
        self.skip_nl();
        let mut args = Vec::new();
        if !matches!(self.kind(), TokenKind::RParen) {
            loop {
                // named arg: id = expr
                args.push(self.parse_expr());
                self.skip_nl();
                if matches!(self.kind(), TokenKind::Comma) {
                    self.bump();
                    self.skip_nl();
                } else {
                    break;
                }
            }
        }
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        args
    }

    fn parse_paren_expr(&mut self) -> Tree {
        let lo = self.span();
        self.bump(); // (
        self.skip_nl();
        if matches!(self.kind(), TokenKind::RParen) {
            self.bump();
            return self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::Literal { lit: Lit::Unit },
            );
        }
        let mut es = vec![self.parse_expr()];
        while matches!(self.kind(), TokenKind::Comma) {
            self.bump();
            self.skip_nl();
            es.push(self.parse_expr());
        }
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        if es.len() == 1 {
            es.pop().unwrap()
        } else {
            // TupleN.apply
            let fun = self.alloc(
                lo,
                TreeKind::Ident {
                    name: format!("Tuple{}", es.len()),
                },
            );
            self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::Apply {
                    fun: Box::new(fun),
                    args: es,
                },
            )
        }
    }

    fn parse_block_expr(&mut self) -> Tree {
        let lo = self.span();
        self.bump(); // {
        self.skip_nl_semi();
        if matches!(self.kind(), TokenKind::Case) {
            let cases = self.parse_cases();
            self.expect("}", |k| matches!(k, TokenKind::RBrace));
            // Partial function: (x => x match cases) encoded as Function with Match
            let sel = self.alloc(
                lo,
                TreeKind::Ident {
                    name: "x$pf".into(),
                },
            );
            let mt = self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::Match {
                    selector: Box::new(sel.clone()),
                    cases,
                },
            );
            let tpt = self.empty(lo);
            let rhs = self.empty(lo);
            let param = self.alloc(
                lo,
                TreeKind::ValDef {
                    mods: Modifiers::new(Flags::PARAM),
                    name: "x$pf".into(),
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            );
            return self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::Function {
                    vparams: vec![param],
                    body: Box::new(mt),
                },
            );
        }
        // `{ x => stat; stat }` — nsc's `ResultExpr`: the lambda body is the
        // rest of the block, not a single expression.
        if let Some(vparams) = self.try_lambda_header() {
            let stats = self.parse_stats_until_rbrace();
            self.expect("}", |k| matches!(k, TokenKind::RBrace));
            let span = lo.merge(self.prev_span());
            let body = block_from_stats(self, span, stats);
            return self.alloc(
                span,
                TreeKind::Function {
                    vparams,
                    body: Box::new(body),
                },
            );
        }
        let stats = self.parse_stats_until_rbrace();
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        block_from_stats(self, lo.merge(self.prev_span()), stats)
    }

    /// A lambda header at the start of a brace block: `x =>`, `x: T =>`,
    /// `(a, b) =>`, `implicit x =>`. Restores the position when there is none.
    fn try_lambda_header(&mut self) -> Option<Vec<Tree>> {
        let saved = self.pos;
        let implicit = matches!(self.kind(), TokenKind::Implicit);
        if implicit {
            self.bump();
        }
        let flags = if implicit {
            Flags::PARAM.with(Flags::IMPLICIT)
        } else {
            Flags::PARAM
        };
        let mut params = Vec::new();
        if matches!(self.kind(), TokenKind::LParen) {
            self.bump();
            self.skip_nl();
            if !matches!(self.kind(), TokenKind::RParen) {
                loop {
                    match self.lambda_param(flags) {
                        Some(p) => params.push(p),
                        None => {
                            self.pos = saved;
                            return None;
                        }
                    }
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::Comma) {
                        self.bump();
                        self.skip_nl();
                        continue;
                    }
                    break;
                }
            }
            if !matches!(self.kind(), TokenKind::RParen) {
                self.pos = saved;
                return None;
            }
            self.bump();
        } else {
            match self.lambda_param(flags) {
                Some(p) => params.push(p),
                None => {
                    self.pos = saved;
                    return None;
                }
            }
        }
        self.skip_nl();
        if !matches!(self.kind(), TokenKind::Arrow) {
            self.pos = saved;
            return None;
        }
        self.bump();
        self.skip_nl_semi();
        Some(params)
    }

    fn lambda_param(&mut self, flags: Flags) -> Option<Tree> {
        let (name, sp) = match self.kind().clone() {
            TokenKind::Ident(n) => {
                let sp = self.span();
                self.bump();
                (n, sp)
            }
            TokenKind::Underscore => {
                let sp = self.span();
                self.bump();
                ("_".to_string(), sp)
            }
            _ => return None,
        };
        let tpt = if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.parse_type()
        } else {
            self.empty(sp)
        };
        let rhs = self.empty(sp);
        Some(self.alloc(
            sp,
            TreeKind::ValDef {
                mods: Modifiers::new(flags),
                name,
                tpt: Box::new(tpt),
                rhs: Box::new(rhs),
            },
        ))
    }

    fn parse_new(&mut self) -> Tree {
        let lo = self.span();
        self.bump();
        self.skip_nl();
        if matches!(self.kind(), TokenKind::LBrace) {
            let (_, _, body) = self.parse_template_body();
            let impl_ = Template {
                parents: vec![],
                self_name: None,
                self_tpt: None,
                body,
                span: lo.merge(self.prev_span()),
            };
            let cls = self.alloc(
                impl_.span,
                TreeKind::ClassDef {
                    mods: Modifiers::new(Flags::SYNTHETIC),
                    name: "$anon".into(),
                    tparams: vec![],
                    ctor_mods: Modifiers::default(),
                    vparamss: vec![],
                    impl_,
                },
            );
            return self.alloc(lo.merge(cls.span), TreeKind::New { tpt: Box::new(cls) });
        }
        let mut parents = vec![self.parse_parent()];
        while matches!(self.peek_non_nl(), TokenKind::With) {
            self.skip_nl();
            self.bump();
            parents.push(self.parse_parent());
        }
        let mut body = vec![];
        let mut has_braces = false;
        if matches!(self.peek_non_nl(), TokenKind::LBrace) {
            self.skip_nl();
            let (_, _, b) = self.parse_template_body();
            body = b;
            has_braces = true;
        }
        if !has_braces && parents.len() == 1 {
            // new C or new C(args) — parent may already be Apply
            match parents.pop().unwrap() {
                t @ Tree {
                    kind: TreeKind::Apply { .. },
                    ..
                } => {
                    // new C(args) => Apply(New(C), args)
                    if let TreeKind::Apply { fun, args } = t.kind {
                        let nw = self.alloc(fun.span, TreeKind::New { tpt: fun });
                        return self.alloc(
                            lo.merge(t.span),
                            TreeKind::Apply {
                                fun: Box::new(nw),
                                args,
                            },
                        );
                    }
                    unreachable!()
                }
                tpt => self.alloc(lo.merge(tpt.span), TreeKind::New { tpt: Box::new(tpt) }),
            }
        } else {
            let impl_ = Template {
                parents,
                self_name: None,
                self_tpt: None,
                body,
                span: lo.merge(self.prev_span()),
            };
            let cls = self.alloc(
                impl_.span,
                TreeKind::ClassDef {
                    mods: Modifiers::new(Flags::SYNTHETIC),
                    name: "$anon".into(),
                    tparams: vec![],
                    ctor_mods: Modifiers::default(),
                    vparamss: vec![],
                    impl_,
                },
            );
            self.alloc(lo.merge(cls.span), TreeKind::New { tpt: Box::new(cls) })
        }
    }

    fn parse_interpolated(&mut self, prefix: String) -> Tree {
        let lo = self.span();
        self.bump(); // InterpStart
        let mut parts = Vec::new();
        let mut args = Vec::new();
        loop {
            match self.kind().clone() {
                TokenKind::StringPart(s) => {
                    parts.push(s);
                    self.bump();
                }
                TokenKind::InterpId(name) => {
                    let sp = self.span();
                    self.bump();
                    args.push(self.alloc(sp, TreeKind::Ident { name }));
                }
                TokenKind::InterpEnd(s) => {
                    parts.push(s);
                    self.bump();
                    break;
                }
                TokenKind::Eof => {
                    self.error_here("unterminated interpolated string");
                    break;
                }
                _ => {
                    // ${ expr }
                    args.push(self.parse_expr());
                }
            }
        }
        self.alloc(
            lo.merge(self.prev_span()),
            TreeKind::InterpolatedString {
                prefix,
                parts,
                args,
            },
        )
    }

    // ------------------------------------------------------------------
    // Patterns
    // ------------------------------------------------------------------

    fn parse_pattern(&mut self) -> Tree {
        // alternatives: p | q
        let t = self.parse_pattern1();
        let mut alts = vec![t.clone()];
        loop {
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Ident(s) if s == "|") {
                self.bump();
                alts.push(self.parse_pattern1());
            } else {
                break;
            }
        }
        if alts.len() == 1 {
            t
        } else {
            self.alloc(
                alts[0].span.merge(self.prev_span()),
                TreeKind::Alternative { trees: alts },
            )
        }
    }

    fn parse_pattern1(&mut self) -> Tree {
        let t = self.parse_pattern2();
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            // Do not parse `A => B` here: the following `=>` starts the case body.
            // Function types in typed patterns need parentheses: `case _: (A => B) =>`.
            // `|` separates alternatives, so it must not read as an infix type
            // (`case _: Int | _: String =>`).
            let tpt = self.parse_pattern_type();
            return self.alloc(
                t.span.merge(tpt.span),
                TreeKind::Typed {
                    expr: Box::new(t),
                    tpt: Box::new(tpt),
                },
            );
        }
        t
    }

    fn parse_pattern2(&mut self) -> Tree {
        // bind: x @ p   or ident
        if matches!(self.kind(), TokenKind::Ident(_)) {
            let saved = self.pos;
            let (name, sp) = self.expect_ident();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::At) {
                self.bump();
                let body = self.parse_pattern3();
                return self.alloc(
                    sp.merge(body.span),
                    TreeKind::Bind {
                        name,
                        body: Box::new(body),
                    },
                );
            }
            self.pos = saved;
        }
        self.parse_pattern3()
    }

    /// `Pattern3 ::= SimplePattern { id [nl] SimplePattern }`. nsc desugars an
    /// infix pattern `p op q` to the extractor call `op(p, q)`, so `h :: t` is
    /// `::(h, t)`. `|` stays alternation and is handled by `parse_pattern`.
    fn parse_pattern3(&mut self) -> Tree {
        self.parse_infix_pattern(0)
    }

    fn parse_infix_pattern(&mut self, min_prec: i32) -> Tree {
        let mut left = self.parse_simple_pattern();
        loop {
            let saved = self.pos;
            if matches!(self.kind(), TokenKind::Newline) {
                self.skip_nl();
            }
            let Some(op) = self.ident_text() else {
                self.pos = saved;
                break;
            };
            // `|` is alternation and `*` closes a `_*` sequence pattern.
            if op == "|" || op == "*" {
                self.pos = saved;
                break;
            }
            let prec = op_precedence(&op);
            if prec < min_prec {
                self.pos = saved;
                break;
            }
            // A word-ident after a newline starts a new pattern, not an infix op.
            if saved != self.pos && !is_operator_name(&op) {
                self.pos = saved;
                break;
            }
            let op_span = self.span();
            self.bump();
            if is_operator_name(&op) {
                self.skip_nl();
            }
            if !self.looks_like_pattern_start() {
                self.pos = saved;
                break;
            }
            // Operators ending in `:` are right-associative (`a :: b :: c`).
            let next_min = if op.ends_with(':') { prec } else { prec + 1 };
            let right = self.parse_infix_pattern(next_min);
            let fun = self.alloc(op_span, TreeKind::Ident { name: op });
            left = self.alloc(
                left.span.merge(right.span),
                TreeKind::Apply {
                    fun: Box::new(fun),
                    args: vec![left, right],
                },
            );
        }
        left
    }

    fn looks_like_pattern_start(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Ident(_)
                | TokenKind::This
                | TokenKind::Underscore
                | TokenKind::LParen
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
                | TokenKind::IntLit(_)
                | TokenKind::LongLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::CharLit(_)
                | TokenKind::StringLit(_)
        )
    }

    fn parse_simple_pattern(&mut self) -> Tree {
        self.skip_nl();
        match self.kind().clone() {
            TokenKind::Underscore => {
                let sp = self.span();
                self.bump();
                let t = self.alloc(sp, TreeKind::Wildcard);
                if matches!(self.kind(), TokenKind::Ident(s) if s == "*") {
                    self.bump();
                    return self.alloc(
                        sp.merge(self.prev_span()),
                        TreeKind::Star { elem: Box::new(t) },
                    );
                }
                t
            }
            TokenKind::Ident(_) | TokenKind::This => {
                let mut t = self.parse_path();
                self.skip_nl();
                if matches!(self.kind(), TokenKind::LParen) {
                    let args = self.parse_pattern_args();
                    t = self.alloc(
                        t.span.merge(self.prev_span()),
                        TreeKind::Apply {
                            fun: Box::new(t),
                            args,
                        },
                    );
                }
                // `_*` is Underscore+star in nsc, but this lexer emits Ident("_") then Ident("*")
                // because `_` followed by an operator character starts an identifier.
                if matches!(&t.kind, TreeKind::Ident { name } if name == "_")
                    && matches!(self.kind(), TokenKind::Ident(s) if s == "*")
                {
                    let sp = t.span;
                    self.bump();
                    let wild = self.alloc(sp, TreeKind::Wildcard);
                    return self.alloc(
                        sp.merge(self.prev_span()),
                        TreeKind::Star {
                            elem: Box::new(wild),
                        },
                    );
                }
                t
            }
            TokenKind::LParen => {
                let lo = self.span();
                self.bump();
                if matches!(self.kind(), TokenKind::RParen) {
                    self.bump();
                    return self.alloc(
                        lo.merge(self.prev_span()),
                        TreeKind::Literal { lit: Lit::Unit },
                    );
                }
                let mut ps = vec![self.parse_pattern()];
                while matches!(self.kind(), TokenKind::Comma) {
                    self.bump();
                    ps.push(self.parse_pattern());
                }
                self.expect(")", |k| matches!(k, TokenKind::RParen));
                if ps.len() == 1 {
                    ps.pop().unwrap()
                } else {
                    let fun = self.alloc(
                        lo,
                        TreeKind::Ident {
                            name: format!("Tuple{}", ps.len()),
                        },
                    );
                    self.alloc(
                        lo.merge(self.prev_span()),
                        TreeKind::Apply {
                            fun: Box::new(fun),
                            args: ps,
                        },
                    )
                }
            }
            TokenKind::True => self.lit_pat(Lit::Boolean(true)),
            TokenKind::False => self.lit_pat(Lit::Boolean(false)),
            TokenKind::Null => self.lit_pat(Lit::Null),
            TokenKind::IntLit(n) => self.lit_pat(Lit::Int(n)),
            TokenKind::LongLit(n) => self.lit_pat(Lit::Long(n)),
            TokenKind::DoubleLit(n) => self.lit_pat(Lit::Double(n)),
            TokenKind::FloatLit(n) => self.lit_pat(Lit::Float(n)),
            TokenKind::CharLit(c) => self.lit_pat(Lit::Char(c)),
            TokenKind::StringLit(s) => self.lit_pat(Lit::String(s)),
            other => {
                self.error_here(format!("expected pattern, found {}", token_name(&other)));
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Wildcard)
            }
        }
    }

    fn lit_pat(&mut self, lit: Lit) -> Tree {
        let sp = self.span();
        self.bump();
        self.alloc(sp, TreeKind::Literal { lit })
    }

    fn parse_pattern_args(&mut self) -> Vec<Tree> {
        self.bump();
        let mut args = Vec::new();
        self.skip_nl();
        if !matches!(self.kind(), TokenKind::RParen) {
            loop {
                args.push(self.parse_pattern_arg());
                self.skip_nl();
                if matches!(self.kind(), TokenKind::Comma) {
                    self.bump();
                    self.skip_nl();
                } else {
                    break;
                }
            }
        }
        self.expect(")", |k| matches!(k, TokenKind::RParen));
        args
    }

    fn parse_pattern_arg(&mut self) -> Tree {
        if matches!(self.kind(), TokenKind::Ident(_)) {
            let saved = self.pos;
            let (name, sp) = self.expect_ident();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Equals) {
                self.bump();
                self.skip_nl();
                let rhs = self.parse_pattern();
                let lhs = self.alloc(sp, TreeKind::Ident { name });
                return self.alloc(
                    sp.merge(rhs.span),
                    TreeKind::Assign {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                );
            }
            self.pos = saved;
        }
        self.parse_pattern()
    }

    /// Scala 2.13 XML literal subset: `<a/>`, `<a></a>`, `<a>t{e}</a>`,
    /// `<a b={e} c="t"/>`, `<p:a xmlns:p="u"/>`, comments, CDATA, PI
    /// (elem / text / splice / attributes / namespaces / prefixed names).
    /// Entity refs are diagnosed, not dropped.
    fn parse_xml_literal(&mut self) -> Tree {
        let lo = self.span();
        self.bump(); // `<`
        self.skip_nl();
        match self.kind().clone() {
            TokenKind::Ident(n) if n.starts_with("!--") || n == "!" || n.starts_with('!') => {
                return self.parse_xml_comment_or_cdata(lo);
            }
            TokenKind::Ident(n) if n.starts_with('?') || n == "?" => {
                return self.parse_xml_pi(lo);
            }
            _ => {}
        }
        self.parse_xml_elem(lo)
    }

    fn parse_xml_comment_or_cdata(&mut self, lo: Span) -> Tree {
        match self.kind().clone() {
            TokenKind::Ident(n) if n.starts_with("!--") => self.parse_xml_comment(lo, &n),
            TokenKind::Ident(n) if n == "!" => {
                self.bump();
                if matches!(self.kind(), TokenKind::LBracket) {
                    self.parse_xml_cdata(lo)
                } else {
                    self.unimplemented(lo.merge(self.span()), "XML comments/CDATA")
                }
            }
            _ => self.unimplemented(lo.merge(self.span()), "XML comments/CDATA"),
        }
    }

    fn parse_xml_comment(&mut self, lo: Span, tok: &str) -> Tree {
        let text_lo = if tok == "!--" {
            self.span().hi.0
        } else {
            self.span().lo.0 + 3
        };
        self.bump();
        loop {
            if self.at_eof() {
                return self.unimplemented(lo.merge(self.span()), "XML comments/CDATA");
            }
            if let TokenKind::Ident(n) = self.kind().clone() {
                if n == "-->" || n.ends_with("-->") {
                    let text_hi = if n == "-->" {
                        self.span().lo.0
                    } else {
                        self.span().hi.0.saturating_sub(3)
                    };
                    let text = self.source_slice(text_lo, text_hi);
                    let end = self.span();
                    self.bump();
                    return self.xml_comment(&text, lo.merge(end));
                }
            }
            self.bump();
        }
    }

    fn parse_xml_cdata(&mut self, lo: Span) -> Tree {
        if !matches!(self.kind(), TokenKind::LBracket) {
            return self.unimplemented(lo.merge(self.span()), "XML comments/CDATA");
        }
        self.bump();
        match self.kind().clone() {
            TokenKind::Ident(n) if n == "CDATA" => {
                self.bump();
            }
            _ => {
                return self.unimplemented(lo.merge(self.span()), "XML comments/CDATA");
            }
        }
        if !matches!(self.kind(), TokenKind::LBracket) {
            return self.unimplemented(lo.merge(self.span()), "XML comments/CDATA");
        }
        let text_lo = self.span().hi.0;
        self.bump();
        loop {
            if self.at_eof() {
                return self.unimplemented(lo.merge(self.span()), "XML comments/CDATA");
            }
            if matches!(self.kind(), TokenKind::RBracket) {
                let first = self.span();
                self.bump();
                if matches!(self.kind(), TokenKind::RBracket) {
                    self.bump();
                    if matches!(self.kind(), TokenKind::Ident(s) if s == ">") {
                        let text = self.source_slice(text_lo, first.lo.0);
                        let end = self.span();
                        self.bump();
                        return self.xml_cdata(&text, lo.merge(end));
                    }
                }
                continue;
            }
            self.bump();
        }
    }

    fn parse_xml_pi(&mut self, lo: Span) -> Tree {
        let n = match self.kind().clone() {
            TokenKind::Ident(n) => n,
            _ => {
                return self.unimplemented(lo.merge(self.span()), "XML processing instructions");
            }
        };
        let (target, text_lo) = if n == "?" {
            self.bump();
            match self.kind().clone() {
                TokenKind::Ident(t) if !is_operator_name(&t) && t != ">" && t != "?>" => {
                    let hi = self.span().hi.0;
                    self.bump();
                    (t, hi)
                }
                _ => {
                    return self
                        .unimplemented(lo.merge(self.span()), "XML processing instructions");
                }
            }
        } else if let Some(rest) = n.strip_prefix('?') {
            if rest.is_empty() || is_operator_name(rest) {
                return self.unimplemented(lo.merge(self.span()), "XML processing instructions");
            }
            let hi = self.span().hi.0;
            self.bump();
            (rest.to_string(), hi)
        } else {
            return self.unimplemented(lo.merge(self.span()), "XML processing instructions");
        };
        loop {
            if self.at_eof() {
                return self.unimplemented(lo.merge(self.span()), "XML processing instructions");
            }
            if let TokenKind::Ident(n) = self.kind().clone() {
                if n == "?>" || n.ends_with("?>") {
                    let text_hi = if n == "?>" {
                        self.span().lo.0
                    } else {
                        self.span().hi.0.saturating_sub(2)
                    };
                    let text = self.source_slice(text_lo, text_hi).trim().to_string();
                    let end = self.span();
                    self.bump();
                    return self.xml_pi(&target, &text, lo.merge(end));
                }
            }
            self.bump();
        }
    }

    fn source_slice(&self, lo: u32, hi: u32) -> String {
        let s = &self.source.src;
        let lo = (lo as usize).min(s.len());
        let hi = (hi as usize).min(s.len()).max(lo);
        s[lo..hi].to_string()
    }

    fn parse_xml_elem(&mut self, lo: Span) -> Tree {
        let mut name = match self.kind().clone() {
            TokenKind::Ident(n)
                if n != ">"
                    && n != "/>"
                    && n != "</"
                    && n != "<"
                    && n != "/"
                    && !is_operator_name(&n) =>
            {
                self.bump();
                n
            }
            _ => {
                return self
                    .unimplemented(lo.merge(self.span()), "XML literal: expected element name");
            }
        };
        self.skip_nl();
        let prefix = if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.skip_nl();
            match self.kind().clone() {
                TokenKind::Ident(local)
                    if local != ">"
                        && local != "/>"
                        && local != "</"
                        && local != "<"
                        && !is_operator_name(&local) =>
                {
                    self.bump();
                    let pre = name;
                    name = local;
                    Some(pre)
                }
                _ => {
                    return self.unimplemented(lo.merge(self.span()), "XML prefixed element names");
                }
            }
        } else {
            None
        };
        let (attrs, xmlns) = self.parse_xml_attrs();
        match self.kind().clone() {
            TokenKind::Ident(n) if n == "/>" => {
                self.bump();
                return self.xml_elem(
                    prefix.as_deref(),
                    &name,
                    attrs,
                    xmlns,
                    Vec::new(),
                    lo.merge(self.prev_span()),
                );
            }
            TokenKind::Ident(n) if n == ">" => {
                self.bump();
            }
            TokenKind::Ident(n) if n == "</" => {
                // `><!--` was one token; attrs already diagnosed comments/CDATA/PI.
            }
            _ => {
                return self.unimplemented(
                    lo.merge(self.span()),
                    "XML literal: expected `>` after element name",
                );
            }
        }
        let mut children = Vec::new();
        loop {
            self.skip_nl();
            match self.kind().clone() {
                TokenKind::Ident(n) if n == "</" => {
                    self.bump();
                    let (cpre, close) = self.parse_xml_close_name();
                    self.skip_nl();
                    if matches!(self.kind(), TokenKind::Ident(s) if s == ">") {
                        self.bump();
                    } else {
                        self.error_here("expected `>` after XML closing tag");
                    }
                    let open = match &prefix {
                        Some(p) => format!("{p}:{name}"),
                        None => name.clone(),
                    };
                    let closed = match &cpre {
                        Some(p) => format!("{p}:{close}"),
                        None => close,
                    };
                    if closed != open {
                        self.error_span(
                            lo.merge(self.prev_span()),
                            format!("XML closing tag `</{closed}>` does not match `<{open}>`"),
                        );
                    }
                    break;
                }
                TokenKind::Ident(n) if n == "<" => {
                    let nested = self.parse_xml_literal();
                    children.push(nested);
                }
                TokenKind::LBrace => {
                    let e = self.parse_block_expr();
                    children.push(self.xml_atom(e));
                }
                TokenKind::Ident(n) if n == ">" || n == "/>" => {
                    return self.unimplemented(self.span(), "XML literal: unexpected `>`");
                }
                TokenKind::Ident(n) if n == "&" || n == "&#" || n.starts_with('&') => {
                    children.push(self.parse_xml_entity());
                }
                TokenKind::Ident(n) => {
                    if let Some(what) = xml_unsupported_markup(&n) {
                        return self.unimplemented(self.span(), what);
                    }
                    self.bump();
                    let mut text = n;
                    while let TokenKind::Ident(more) = self.kind().clone() {
                        if more == "<"
                            || more == "</"
                            || more == ">"
                            || more == "/>"
                            || is_operator_name(&more)
                            || xml_unsupported_markup(&more).is_some()
                        {
                            break;
                        }
                        text.push_str(&more);
                        self.bump();
                    }
                    children.push(self.xml_text(&text, lo));
                }
                TokenKind::IntLit(i) => {
                    self.bump();
                    children.push(self.xml_text(&i.to_string(), lo));
                }
                TokenKind::StringLit(s) => {
                    self.bump();
                    children.push(self.xml_text(&s, lo));
                }
                TokenKind::Eof => {
                    return self
                        .unimplemented(lo.merge(self.span()), "XML literal: unclosed element");
                }
                _ => {
                    return self.unimplemented(self.span(), "XML literal content");
                }
            }
        }
        self.xml_elem(
            prefix.as_deref(),
            &name,
            attrs,
            xmlns,
            children,
            lo.merge(self.prev_span()),
        )
    }

    fn parse_xml_close_name(&mut self) -> (Option<String>, String) {
        let first = match self.kind().clone() {
            TokenKind::Ident(c) if c != ">" && c != "</" && c != "<" && !is_operator_name(&c) => {
                self.bump();
                c
            }
            _ => {
                self.error_here("expected XML closing tag name");
                return (None, String::new());
            }
        };
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.skip_nl();
            match self.kind().clone() {
                TokenKind::Ident(local) if local != ">" && !is_operator_name(&local) => {
                    self.bump();
                    (Some(first), local)
                }
                _ => {
                    self.error_here("expected XML closing tag local name");
                    (Some(first), String::new())
                }
            }
        } else {
            (None, first)
        }
    }

    fn parse_xml_attr_value(&mut self) -> Option<Tree> {
        match self.kind().clone() {
            TokenKind::LBrace => Some(self.parse_block_expr()),
            TokenKind::StringLit(s) => {
                let sp = self.span();
                self.bump();
                Some(self.alloc(
                    sp,
                    TreeKind::Literal {
                        lit: Lit::String(s),
                    },
                ))
            }
            _ => {
                let _ = self.unimplemented(self.span(), "XML attribute value");
                self.skip_xml_attr_tail();
                None
            }
        }
    }

    /// Unprefixed `b={e}` / `c="t"`, prefixed `p:b=…`, and `xmlns:p="uri"` /
    /// default `xmlns="uri"`. Entity refs stay diagnosed.
    fn parse_xml_attrs(&mut self) -> (Vec<XmlAttr>, Vec<(Option<String>, Tree)>) {
        let mut attrs = Vec::new();
        let mut xmlns = Vec::new();
        loop {
            self.skip_nl();
            match self.kind().clone() {
                TokenKind::Ident(n) if n == ">" || n == "/>" || n == "</" || n == "<" => break,
                TokenKind::Ident(n) if n == "&" || n == "&#" || n.starts_with('&') => break,
                TokenKind::Eof => break,
                TokenKind::Ident(n) if xml_unsupported_markup(&n).is_some() => {
                    let what = xml_unsupported_markup(&n).unwrap();
                    let sp = self.span();
                    self.bump();
                    let _ = self.unimplemented(sp, what);
                    loop {
                        match self.kind().clone() {
                            TokenKind::Ident(t) if t == "</" || t == ">" || t == "/>" => break,
                            TokenKind::Eof => break,
                            _ => {
                                self.bump();
                            }
                        }
                    }
                    break;
                }
                TokenKind::Ident(n) if !is_operator_name(&n) => {
                    let lo = self.span();
                    self.bump();
                    self.skip_nl();
                    if n == "xmlns" {
                        let prefix = if matches!(self.kind(), TokenKind::Colon) {
                            self.bump();
                            self.skip_nl();
                            match self.kind().clone() {
                                TokenKind::Ident(p) if !is_operator_name(&p) => {
                                    self.bump();
                                    Some(p)
                                }
                                _ => {
                                    let _ = self.unimplemented(
                                        lo.merge(self.span()),
                                        "XML namespace prefix",
                                    );
                                    self.skip_xml_attr_tail();
                                    continue;
                                }
                            }
                        } else {
                            None
                        };
                        self.skip_nl();
                        if !matches!(self.kind(), TokenKind::Equals) {
                            let _ =
                                self.unimplemented(lo.merge(self.span()), "XML namespace binding");
                            self.skip_xml_attr_tail();
                            continue;
                        }
                        self.bump();
                        self.skip_nl();
                        if let Some(value) = self.parse_xml_attr_value() {
                            xmlns.push((prefix, value));
                        }
                        continue;
                    }
                    let prefix = if matches!(self.kind(), TokenKind::Colon) {
                        self.bump();
                        self.skip_nl();
                        match self.kind().clone() {
                            TokenKind::Ident(key) if !is_operator_name(&key) => {
                                self.bump();
                                Some((n.clone(), key))
                            }
                            _ => {
                                let _ = self
                                    .unimplemented(lo.merge(self.span()), "XML prefixed attribute");
                                self.skip_xml_attr_tail();
                                continue;
                            }
                        }
                    } else {
                        None
                    };
                    self.skip_nl();
                    if !matches!(self.kind(), TokenKind::Equals) {
                        let _ =
                            self.unimplemented(lo.merge(self.span()), "XML attributes/namespaces");
                        self.skip_xml_attr_tail();
                        continue;
                    }
                    self.bump();
                    self.skip_nl();
                    let Some(value) = self.parse_xml_attr_value() else {
                        continue;
                    };
                    if let Some((pre, key)) = prefix {
                        attrs.push(XmlAttr {
                            prefix: Some(pre),
                            key,
                            value,
                        });
                    } else {
                        attrs.push(XmlAttr {
                            prefix: None,
                            key: n,
                            value,
                        });
                    }
                }
                _ => {
                    let _ = self.unimplemented(self.span(), "XML attributes/namespaces");
                    loop {
                        match self.kind() {
                            TokenKind::Ident(n)
                                if n == ">" || n == "/>" || n == "</" || n == "<" =>
                            {
                                break;
                            }
                            TokenKind::Eof => break,
                            _ => {
                                self.bump();
                            }
                        }
                    }
                    break;
                }
            }
        }
        (attrs, xmlns)
    }

    fn skip_xml_attr_tail(&mut self) {
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Colon) {
            self.bump();
            self.skip_nl();
            if matches!(self.kind(), TokenKind::Ident(_)) {
                self.bump();
            }
        }
        self.skip_nl();
        if matches!(self.kind(), TokenKind::Equals) {
            self.bump();
            self.skip_nl();
            match self.kind().clone() {
                TokenKind::LBrace => {
                    let _ = self.parse_block_expr();
                }
                TokenKind::StringLit(_) | TokenKind::Ident(_) | TokenKind::IntLit(_) => {
                    self.bump();
                }
                _ => {}
            }
        }
    }

    fn xml_path(&mut self, names: &[&str], span: Span) -> Tree {
        let mut t = self.alloc(
            span,
            TreeKind::Ident {
                name: names[0].into(),
            },
        );
        for n in &names[1..] {
            t = self.alloc(
                span,
                TreeKind::Select {
                    qual: Box::new(t),
                    name: (*n).into(),
                },
            );
        }
        t
    }

    fn xml_new(&mut self, cls: Tree, args: Vec<Tree>, span: Span) -> Tree {
        let nw = self.alloc(cls.span, TreeKind::New { tpt: Box::new(cls) });
        self.alloc(
            span,
            TreeKind::Apply {
                fun: Box::new(nw),
                args,
            },
        )
    }

    fn xml_text(&mut self, s: &str, span: Span) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "Text"], span);
        let lit = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(s.into()),
            },
        );
        self.xml_new(cls, vec![lit], span)
    }

    fn xml_entity_ref(&mut self, name: &str, span: Span) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "EntityRef"], span);
        let lit = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(name.into()),
            },
        );
        self.xml_new(cls, vec![lit], span)
    }

    /// nsc `content_AMP`: named refs become `EntityRef(name)`; `&#N;` / `&#xN;`
    /// become `Text` of the decoded character. Unknown names diagnose.
    fn parse_xml_entity(&mut self) -> Tree {
        let lo = self.span();
        let tok = match self.kind().clone() {
            TokenKind::Ident(n) => n,
            _ => {
                return self.unimplemented(lo, "XML entity references");
            }
        };
        self.bump();
        if tok == "&#" || tok.starts_with("&#") {
            return self.parse_xml_char_ref(lo, &tok);
        }
        let name = if tok == "&" {
            match self.kind().clone() {
                TokenKind::Ident(n)
                    if n != ";" && n != "<" && n != ">" && !is_operator_name(&n) =>
                {
                    self.bump();
                    n
                }
                _ => {
                    return self.unimplemented(lo.merge(self.span()), "XML entity references");
                }
            }
        } else if let Some(rest) = tok.strip_prefix('&') {
            rest.to_string()
        } else {
            return self.unimplemented(lo, "XML entity references");
        };
        if !matches!(self.kind(), TokenKind::Semi) {
            return self.unimplemented(lo.merge(self.span()), "XML entity references");
        }
        let end = self.span();
        self.bump();
        if xml_predefined_entity(&name) {
            self.xml_entity_ref(&name, lo.merge(end))
        } else {
            self.unimplemented(
                lo.merge(end),
                format!("XML entity references: unknown `&{name};`"),
            )
        }
    }

    fn parse_xml_char_ref(&mut self, lo: Span, tok: &str) -> Tree {
        let (hex, rest) = if tok == "&#" {
            (false, String::new())
        } else if let Some(r) = tok.strip_prefix("&#") {
            if r.starts_with('x') || r.starts_with('X') {
                (true, r[1..].to_string())
            } else {
                (false, r.to_string())
            }
        } else {
            return self.unimplemented(lo, "XML entity references");
        };
        let mut hex = hex;
        let mut digits = rest;
        if digits.is_empty() {
            match self.kind().clone() {
                TokenKind::IntLit(n) if !hex => {
                    digits = n.to_string();
                    self.bump();
                }
                TokenKind::Ident(n) if !hex && (n.starts_with('x') || n.starts_with('X')) => {
                    hex = true;
                    digits = n[1..].to_string();
                    self.bump();
                }
                TokenKind::Ident(n) if hex => {
                    digits = n;
                    self.bump();
                }
                _ => {
                    return self.unimplemented(lo.merge(self.span()), "XML entity references");
                }
            }
        }
        if !matches!(self.kind(), TokenKind::Semi) {
            return self.unimplemented(lo.merge(self.span()), "XML entity references");
        }
        let end = self.span();
        self.bump();
        let radix = if hex { 16 } else { 10 };
        match u32::from_str_radix(&digits, radix)
            .ok()
            .and_then(char::from_u32)
        {
            Some(ch) => self.xml_text(&ch.to_string(), lo.merge(end)),
            None => self.unimplemented(lo.merge(end), "XML entity references"),
        }
    }

    fn xml_comment(&mut self, s: &str, span: Span) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "Comment"], span);
        let lit = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(s.into()),
            },
        );
        self.xml_new(cls, vec![lit], span)
    }

    fn xml_cdata(&mut self, s: &str, span: Span) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "PCData"], span);
        let lit = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(s.into()),
            },
        );
        self.xml_new(cls, vec![lit], span)
    }

    fn xml_pi(&mut self, target: &str, text: &str, span: Span) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "ProcInstr"], span);
        let t = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(target.into()),
            },
        );
        let b = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(text.into()),
            },
        );
        self.xml_new(cls, vec![t, b], span)
    }

    fn xml_atom(&mut self, e: Tree) -> Tree {
        let span = e.span;
        let cls = self.xml_path(&["scala", "xml", "Atom"], span);
        self.xml_new(cls, vec![e], span)
    }

    fn xml_cons(&mut self, head: Tree, tail: Tree, span: Span) -> Tree {
        let sel = self.alloc(
            span,
            TreeKind::Select {
                qual: Box::new(tail),
                name: "::".into(),
            },
        );
        self.alloc(
            span,
            TreeKind::Apply {
                fun: Box::new(sel),
                args: vec![head],
            },
        )
    }

    /// Nested `UnprefixedAttribute` / `PrefixedAttribute` chain ending at
    /// `scala.xml.Null` (nsc `$md`). Unprefixed ctor args are unchanged.
    fn xml_attr_chain(&mut self, attrs: Vec<XmlAttr>, span: Span) -> Tree {
        let mut acc = self.xml_path(&["scala", "xml", "Null"], span);
        for attr in attrs.into_iter().rev() {
            let k = self.alloc(
                span,
                TreeKind::Literal {
                    lit: Lit::String(attr.key),
                },
            );
            acc = if let Some(pre) = attr.prefix {
                let cls = self.xml_path(&["scala", "xml", "PrefixedAttribute"], span);
                let p = self.alloc(
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(pre),
                    },
                );
                self.xml_new(cls, vec![p, k, attr.value, acc], span)
            } else {
                let cls = self.xml_path(&["scala", "xml", "UnprefixedAttribute"], span);
                self.xml_new(cls, vec![k, attr.value, acc], span)
            };
        }
        acc
    }

    /// nsc `$tmpscope = new NamespaceBinding(prefix, uri, $tmpscope)` from TopScope.
    fn xml_scope(&mut self, xmlns: Vec<(Option<String>, Tree)>, span: Span) -> Tree {
        let mut acc = self.xml_path(&["scala", "xml", "TopScope"], span);
        for (prefix, uri) in xmlns {
            let cls = self.xml_path(&["scala", "xml", "NamespaceBinding"], span);
            let pre = match prefix {
                Some(p) => self.alloc(
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(p),
                    },
                ),
                None => self.alloc(span, TreeKind::Literal { lit: Lit::Null }),
            };
            acc = self.xml_new(cls, vec![pre, uri, acc], span);
        }
        acc
    }

    /// `new scala.xml.Elem(prefix|null, label, attrs, scope, true, children)`
    fn xml_elem(
        &mut self,
        prefix: Option<&str>,
        label: &str,
        attrs: Vec<XmlAttr>,
        xmlns: Vec<(Option<String>, Tree)>,
        children: Vec<Tree>,
        span: Span,
    ) -> Tree {
        let cls = self.xml_path(&["scala", "xml", "Elem"], span);
        let prefix = match prefix {
            Some(p) => self.alloc(
                span,
                TreeKind::Literal {
                    lit: Lit::String(p.into()),
                },
            ),
            None => self.alloc(span, TreeKind::Literal { lit: Lit::Null }),
        };
        let lab = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::String(label.into()),
            },
        );
        let attrs = self.xml_attr_chain(attrs, span);
        let scope = self.xml_scope(xmlns, span);
        let min = self.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::Boolean(true),
            },
        );
        let mut acc = self.xml_path(&["Nil"], span);
        for ch in children.into_iter().rev() {
            acc = self.xml_cons(ch, acc, span);
        }
        self.xml_new(cls, vec![prefix, lab, attrs, scope, min, acc], span)
    }
}

fn xml_predefined_entity(name: &str) -> bool {
    matches!(name, "amp" | "lt" | "gt" | "quot" | "apos")
}

/// Leftover glued markup (`><!--` as one Ident). Named/numeric entities in
/// element content go through `parse_xml_entity`.
fn xml_unsupported_markup(n: &str) -> Option<&'static str> {
    if n.contains("<!--")
        || n.contains("<![")
        || n.contains("<!")
        || n.starts_with("!--")
        || n.contains("![CDATA")
    {
        Some("XML comments/CDATA")
    } else if n.contains("<?") {
        Some("XML processing instructions")
    } else if n == "&" || n.starts_with('&') {
        Some("XML entity references")
    } else {
        None
    }
}

fn flatten_val_block(t: Tree) -> Vec<Tree> {
    match t.kind {
        TreeKind::Block { stats, expr }
            if !stats.is_empty()
                && stats
                    .iter()
                    .all(|s| matches!(s.kind, TreeKind::ValDef { .. }))
                && matches!(expr.kind, TreeKind::ValDef { .. }) =>
        {
            let mut all = stats;
            all.push(*expr);
            all
        }
        kind => vec![Tree { kind, ..t }],
    }
}

fn is_unit_tuple(t: &Tree) -> bool {
    matches!(&t.kind, TreeKind::Ident { name } if name == "Unit")
        || matches!(&t.kind, TreeKind::Literal { lit: Lit::Unit })
}

fn is_def_start(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Class
            | TokenKind::Object
            | TokenKind::Trait
            | TokenKind::Val
            | TokenKind::Var
            | TokenKind::Def
            | TokenKind::TypeKw
            | TokenKind::Package
            | TokenKind::Import
    )
}

fn is_mod_or_def_start(k: &TokenKind) -> bool {
    is_def_start(k)
        || matches!(
            k,
            TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Abstract
                | TokenKind::Final
                | TokenKind::Sealed
                | TokenKind::Implicit
                | TokenKind::Lazy
                | TokenKind::Override
                | TokenKind::Case
                | TokenKind::At
        )
}

fn token_name(k: &TokenKind) -> String {
    match k {
        TokenKind::Eof => "end of file".into(),
        TokenKind::Ident(s) => format!("`{s}`"),
        TokenKind::Newline => "newline".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn is_placeholder_wildcard(t: &Tree, params: &[Tree]) -> bool {
    let Some(last) = params.last() else {
        return false;
    };
    let Some(pname) = last.name() else {
        return false;
    };
    match &t.kind {
        TreeKind::Ident { name } => name == pname,
        TreeKind::Typed { expr, .. } => is_placeholder_wildcard(expr, params),
        _ => false,
    }
}

fn expr_to_params(t: Tree) -> Vec<Tree> {
    match t.kind {
        TreeKind::Ident { name } => vec![Tree {
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ..t
        }],
        TreeKind::Wildcard => vec![Tree {
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name: "_".into(),
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ..t
        }],
        TreeKind::Typed { expr, tpt } => {
            let name = expr.name().unwrap_or("_").to_string();
            vec![Tree {
                kind: TreeKind::ValDef {
                    mods: Modifiers::new(Flags::PARAM),
                    name,
                    tpt,
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                },
                ..*expr
            }]
        }
        TreeKind::Apply { args, .. } => {
            // tuple
            args.into_iter().flat_map(expr_to_params).collect()
        }
        TreeKind::Literal { lit: Lit::Unit } => vec![],
        _ => vec![Tree {
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name: "_".into(),
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ..t
        }],
    }
}

fn refinement_has_impl(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::ValDef { rhs, .. } | TreeKind::DefDef { rhs, .. } => !rhs.is_empty(),
        _ => false,
    }
}

fn block_from_stats(p: &mut Parser, span: Span, mut stats: Vec<Tree>) -> Tree {
    if stats.is_empty() {
        return p.alloc(span, TreeKind::Literal { lit: Lit::Unit });
    }
    let expr = stats.pop().unwrap();
    if stats.is_empty() {
        expr
    } else {
        p.alloc(
            span.merge(expr.span),
            TreeKind::Block {
                stats,
                expr: Box::new(expr),
            },
        )
    }
}

fn desugar_for(
    p: &mut Parser,
    lo: Span,
    enums: Vec<Enumerator>,
    body: Tree,
    is_yield: bool,
) -> Tree {
    if enums.is_empty() {
        p.error_span(lo, "empty for-comprehension");
        return body;
    }
    fn pat_to_param(pat: Tree) -> Tree {
        let name = match &pat.kind {
            TreeKind::Ident { name } => name.clone(),
            TreeKind::Bind { name, .. } => name.clone(),
            TreeKind::Wildcard => "_".into(),
            _ => "x$for".into(),
        };
        Tree {
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            span: pat.span,
            id: pat.id,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        }
    }
    /// A generator pattern that always matches: a variable, `_`, or a tuple of
    /// those. nsc filters the others, and so does `filter_lambda` below.
    fn is_irrefutable(pat: &Tree) -> bool {
        match &pat.kind {
            TreeKind::Ident { .. } | TreeKind::Wildcard => true,
            TreeKind::Bind { body, .. } => is_irrefutable(body),
            TreeKind::Apply { fun, args } => {
                matches!(&fun.kind, TreeKind::Ident { name } if name.starts_with("Tuple"))
                    && args.iter().all(is_irrefutable)
            }
            _ => false,
        }
    }
    fn lambda(p: &mut Parser, pat: Tree, body: Tree) -> Tree {
        let span = pat.span.merge(body.span);
        if !matches!(
            &pat.kind,
            TreeKind::Ident { .. } | TreeKind::Bind { .. } | TreeKind::Wildcard
        ) {
            // `for ((a, b) <- xs) yield a` — bind the element and match it,
            // which is what nsc's pattern-matching anonymous function does.
            p.placeholder_id += 1;
            let name = format!("x$for{}", p.placeholder_id);
            let sel = p.alloc(pat.span, TreeKind::Ident { name: name.clone() });
            let guard = p.empty(pat.span);
            let pat_span = pat.span;
            let m = p.alloc(
                span,
                TreeKind::Match {
                    selector: Box::new(sel),
                    cases: vec![CaseDef {
                        pat,
                        guard,
                        body,
                        span,
                    }],
                },
            );
            let tpt = p.empty(pat_span);
            let rhs = p.empty(pat_span);
            let v = p.alloc(
                pat_span,
                TreeKind::ValDef {
                    mods: Modifiers::new(Flags::PARAM),
                    name,
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            );
            return p.alloc(
                span,
                TreeKind::Function {
                    vparams: vec![v],
                    body: Box::new(m),
                },
            );
        }
        let v = pat_to_param(pat);
        p.alloc(
            span,
            TreeKind::Function {
                vparams: vec![v],
                body: Box::new(body),
            },
        )
    }

    /// `{ x => x match { case pat => true; case _ => false } }` for a refutable
    /// generator pattern, as nsc's `withFilter` insertion does.
    fn filter_lambda(p: &mut Parser, pat: &Tree) -> Tree {
        let span = pat.span;
        p.placeholder_id += 1;
        let name = format!("x$forf{}", p.placeholder_id);
        let sel = p.alloc(span, TreeKind::Ident { name: name.clone() });
        let yes = p.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::Boolean(true),
            },
        );
        let no = p.alloc(
            span,
            TreeKind::Literal {
                lit: Lit::Boolean(false),
            },
        );
        let g1 = p.empty(span);
        let g2 = p.empty(span);
        let wild = p.alloc(span, TreeKind::Wildcard);
        let m = p.alloc(
            span,
            TreeKind::Match {
                selector: Box::new(sel),
                cases: vec![
                    CaseDef {
                        pat: pat.clone(),
                        guard: g1,
                        body: yes,
                        span,
                    },
                    CaseDef {
                        pat: wild,
                        guard: g2,
                        body: no,
                        span,
                    },
                ],
            },
        );
        let tpt = p.empty(span);
        let rhs = p.empty(span);
        let v = p.alloc(
            span,
            TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name,
                tpt: Box::new(tpt),
                rhs: Box::new(rhs),
            },
        );
        p.alloc(
            span,
            TreeKind::Function {
                vparams: vec![v],
                body: Box::new(m),
            },
        )
    }
    // Work from the last enumerator backwards.
    let mut acc = body;
    for (i, e) in enums.into_iter().rev().enumerate() {
        let last = i == 0;
        let mut rhs = e.rhs;
        if let Some(g) = e.guard {
            let pred = lambda(p, dummy_ident_from(&e.pat), g);
            let sel = p.alloc(
                rhs.span,
                TreeKind::Select {
                    qual: Box::new(rhs),
                    name: "withFilter".into(),
                },
            );
            rhs = p.alloc(
                sel.span,
                TreeKind::Apply {
                    fun: Box::new(sel),
                    args: vec![pred],
                },
            );
        }
        if e.is_val {
            // `y = e` => map { x => val y = e; acc }
            let tpt = p.empty(e.pat.span);
            let vd = p.alloc(
                e.pat.span,
                TreeKind::ValDef {
                    mods: Modifiers::default(),
                    name: e.pat.name().unwrap_or("y$for").to_string(),
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            );
            acc = p.alloc(
                vd.span.merge(acc.span),
                TreeKind::Block {
                    stats: vec![vd],
                    expr: Box::new(acc),
                },
            );
            continue;
        }
        if !e.is_val && !is_irrefutable(&e.pat) {
            let pred = filter_lambda(p, &e.pat);
            let sel = p.alloc(
                rhs.span,
                TreeKind::Select {
                    qual: Box::new(rhs),
                    name: "withFilter".into(),
                },
            );
            rhs = p.alloc(
                sel.span,
                TreeKind::Apply {
                    fun: Box::new(sel),
                    args: vec![pred],
                },
            );
        }
        let method = if last {
            if is_yield {
                "map"
            } else {
                "foreach"
            }
        } else if is_yield {
            "flatMap"
        } else {
            "foreach"
        };
        let fun = lambda(p, e.pat, acc);
        let sel = p.alloc(
            rhs.span,
            TreeKind::Select {
                qual: Box::new(rhs),
                name: method.into(),
            },
        );
        acc = p.alloc(
            lo.merge(fun.span),
            TreeKind::Apply {
                fun: Box::new(sel),
                args: vec![fun],
            },
        );
    }
    acc
}

fn dummy_ident_from(pat: &Tree) -> Tree {
    let name = pat.name().unwrap_or("_").to_string();
    Tree {
        id: pat.id,
        span: pat.span,
        kind: TreeKind::Ident { name },
        ty: Type::NoType,
        sym: SymbolId::NONE,
        postfix: false,
    }
}

/// Variables a pattern binds, in source order (`case a :: Rest(b) =>` gives
/// `a`, `b`). Used to desugar pattern definitions and generators.
fn pattern_bound_names(pat: &Tree, out: &mut Vec<String>) {
    match &pat.kind {
        TreeKind::Ident { name } => {
            if name != "_"
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || c == '_')
            {
                out.push(name.clone());
            }
        }
        TreeKind::Bind { name, body } => {
            out.push(name.clone());
            pattern_bound_names(body, out);
        }
        TreeKind::Apply { args, .. } => {
            for a in args {
                pattern_bound_names(a, out);
            }
        }
        TreeKind::Typed { expr, .. } => pattern_bound_names(expr, out),
        TreeKind::Star { elem } => pattern_bound_names(elem, out),
        TreeKind::Alternative { trees } => {
            if let Some(first) = trees.first() {
                pattern_bound_names(first, out);
            }
        }
        _ => {}
    }
}
