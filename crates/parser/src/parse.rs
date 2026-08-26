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

/// Language-level annotations nsc cares about in this subset.
fn annotation_supported(path: &str) -> bool {
    matches!(
        path,
        "tailrec"
            | "annotation.tailrec"
            | "scala.annotation.tailrec"
            | "deprecated"
            | "scala.deprecated"
            | "Override"
            | "java.lang.Override"
            | "Deprecated"
            | "java.lang.Deprecated"
    )
}

struct Parser<'a> {
    source: &'a SourceFile,
    file_index: usize,
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    next_id: u32,
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
            stats.push(self.parse_tmpl_or_def());
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
        if matches!(self.kind(), TokenKind::Extends) {
            self.bump();
            self.skip_nl();
            parents.push(self.parse_parent());
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
        } else if is_trait && !matches!(self.kind(), TokenKind::LBrace) {
            // trait T  (no body)
        }
        let (self_name, self_tpt, body) = if matches!(self.peek_non_nl(), TokenKind::LBrace) {
            self.skip_nl();
            self.parse_template_body()
        } else {
            (None, None, vec![])
        };
        Template {
            parents,
            self_name,
            self_tpt,
            body,
            span: lo.merge(self.prev_span()),
        }
    }

    fn parse_parent(&mut self) -> Tree {
        // AnnotType [ArgumentExprs]
        let tpt = self.parse_annot_type();
        self.skip_nl();
        if matches!(self.kind(), TokenKind::LParen) {
            let args = self.parse_arg_exprs();
            self.alloc(
                tpt.span.merge(self.prev_span()),
                TreeKind::Apply {
                    fun: Box::new(tpt),
                    args,
                },
            )
        } else {
            tpt
        }
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
        let mut stats = Vec::new();
        loop {
            self.skip_nl_semi();
            if self.at_eof() || matches!(self.kind(), TokenKind::RBrace) {
                break;
            }
            stats.push(self.parse_block_stat());
            if !self.accept_separator()
                && !matches!(self.kind(), TokenKind::RBrace | TokenKind::Eof)
            {
                if !is_def_start(self.kind()) && !matches!(self.kind(), TokenKind::Case) {
                    // might be ok — next token starts next stat
                }
            }
        }
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
        // If it's a true pattern (not a simple ident), wrap.
        if matches!(&pat.kind, TreeKind::Ident { .. } | TreeKind::Wildcard) {
            self.alloc(
                lo.merge(self.prev_span()),
                TreeKind::ValDef {
                    mods,
                    name,
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                },
            )
        } else {
            // `val (a,b) = e` — emit ValDef with pattern name plus note
            let span = lo.merge(self.prev_span());
            // Keep the pattern in the name field encoded; also emit nested vals
            // as a Block assigned from Match. Desugar:
            // val <tmp> = rhs; val a = <tmp>._1 ...  too heavy. Keep as ValDef
            // named "<pat>" with the pattern tree in tpt? Better: use Bind.
            self.alloc(
                span,
                TreeKind::ValDef {
                    mods,
                    name: "<pat>".into(),
                    tpt: Box::new(pat),
                    rhs: Box::new(rhs),
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

    fn parse_infix_type(&mut self) -> Tree {
        let mut t = self.parse_compound_type();
        loop {
            // infix type: T ident T  (ident not nl-separated in a way that starts a val)
            if !matches!(self.kind(), TokenKind::Ident(_)) {
                break;
            }
            // Don't treat following ident as infix if it's a def start on a new "statement"
            // In types, `T Either U` is infix. If next is ident and then a type, take it.
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
                    let sp = self.span();
                    let _ = self.parse_val_def(Modifiers::default());
                    let span = sp.merge(self.prev_span());
                    self.error_span(span, "unimplemented type: structural var members");
                    decls.push(self.alloc(
                        span,
                        TreeKind::Unimplemented {
                            what: "structural var members".into(),
                        },
                    ));
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
        loop {
            if matches!(self.kind(), TokenKind::Dot) {
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
        self.parse_expr1()
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
                    let vparams = expr_to_params(t.clone());
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
                    let tpt = self.parse_type();
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
            TokenKind::Underscore => {
                let sp = self.span();
                self.bump();
                self.alloc(sp, TreeKind::Wildcard)
            }
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
        let stats = self.parse_stats_until_rbrace();
        self.expect("}", |k| matches!(k, TokenKind::RBrace));
        block_from_stats(self, lo.merge(self.prev_span()), stats)
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
            let tpt = self.parse_infix_type();
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

    fn parse_pattern3(&mut self) -> Tree {
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
    fn lambda(p: &mut Parser, pat: Tree, body: Tree) -> Tree {
        let span = pat.span.merge(body.span);
        let v = pat_to_param(pat);
        p.alloc(
            span,
            TreeKind::Function {
                vparams: vec![v],
                body: Box::new(body),
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
