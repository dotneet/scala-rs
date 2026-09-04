//! kind-projector's type-lambda syntax, behind `-Ykind-projector`.
//!
//! [kind-projector](https://github.com/typelevel/kind-projector) is a compiler
//! *plugin*, not Scala. nsc without it rejects everything in this file, and so
//! does this compiler unless the flag is given: the point of the flag is that
//! the rejection stays correct by default, because "compiles what nsc
//! compiles" is the claim being made everywhere else.
//!
//! The plugin runs right after the parser and rewrites type trees, so this is
//! a purely syntactic pass over the trees the type parser has just built. What
//! it produces is exactly what nsc's `-Xprint:kind-projector` shows for the
//! plugin, a structural type lambda:
//!
//! ```text
//! Either[Int, *]        ~>  AnyRef { type Λ$[β$0$] = Either[Int, β$0$] }#Λ$
//! λ[α => F[G[α]]]       ~>  AnyRef { type Λ$[α] = F[G[α]] }#Λ$
//! λ[(α, β) => E[β, α]]  ~>  AnyRef { type Λ$[α, β] = E[β, α] }#Λ$
//! ```
//!
//! (the `AnyRef` parent is left off here -- `{ type Λ$[…] = … }#Λ$` is the same
//! type and is the spelling `agent/typelambda` already made work.)
//!
//! Two rules were read off the plugin rather than guessed, both checked
//! against `scalac -Xplugin:kind-projector_2.13.16-0.13.3.jar
//! -Xprint:kind-projector`:
//!
//! * A `*` binds to the **innermost enclosing type application**, not the
//!   outermost. `Either[Int, List[*]]` is `Either[Int, [a] => List[a]]`, not
//!   `[a] => Either[Int, List[a]]`. Because the parser builds applications
//!   bottom up, doing the rewrite as each one is finished gets this for free.
//! * A function type counts as an application of `FunctionN`, so `A => *` is
//!   `[b] => A => b` and `* => *` is `[a, b] => a => b`.
//!
//! The generated parameter names follow the plugin's too (`α$0$`, `β$1$`, …:
//! a Greek letter chosen by the *position* of the placeholder in the
//! application, and a counter that runs over the compilation unit), so that a
//! diagnostic naming one reads the way nsc's does.

use scala_rs_span::Span;

use crate::ast::{Flags, Modifiers, Tree, TreeKind};

use super::Parser;

/// The name the plugin gives the refinement's type member is `Λ$` for every
/// lambda: nsc tells two of them apart by symbol. This compiler's refinement
/// machinery matches a member of a refinement *by name*
/// (`symbol::subst_refine_aliases`), so a lambda whose body mentions another
/// lambda would substitute one into the other and, when that body is the one
/// being substituted into, never stop -- 512 MB of stack, and cats went down
/// with `errors=0 classes=0`. So the name carries the file and a counter.
///
/// Unicode `Λ` and `$` are both legal in an identifier here and no source
/// spells this by hand, which is the property the plugin's name has too.
fn lambda_name(file: usize, n: u32) -> String {
    format!("Λ${file}${n}")
}

/// Positional names for placeholder parameters, as the plugin picks them.
/// `λ` is left out: it is the plugin's own keyword.
const GREEK: [&str; 23] = [
    "α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ", "υ",
    "φ", "χ", "ψ", "ω",
];

/// `*`, `+*` and `-*` (one identifier each: `+` and `*` are both operator
/// characters, so the lexer glues them). Returns the variance flag.
fn placeholder_name(name: &str) -> Option<Flags> {
    match name {
        "*" => Some(Flags::EMPTY),
        "+*" => Some(Flags::EMPTY.with(Flags::COVARIANT)),
        "-*" => Some(Flags::EMPTY.with(Flags::CONTRAVARIANT)),
        _ => None,
    }
}

/// A placeholder argument: `*` / `+*` / `-*`, or the higher-kinded form
/// `*[_]` (`EitherT[*[_], Int, *]`). Returns its variance and its own type
/// parameters.
fn placeholder_arg(t: &Tree) -> Option<(Flags, &[Tree])> {
    match &t.kind {
        TreeKind::Ident { name } => placeholder_name(name).map(|v| (v, &[][..])),
        TreeKind::AppliedTypeTree { tpt, args } => match &tpt.kind {
            TreeKind::Ident { name } => placeholder_name(name).map(|v| (v, &args[..])),
            _ => None,
        },
        _ => None,
    }
}

impl Parser<'_> {
    /// `-Ykind-projector` is on.
    fn kp(&self) -> bool {
        self.opts.kind_projector
    }

    /// A fresh parameter name for the placeholder at argument index `i`.
    fn kp_fresh(&mut self, i: usize) -> String {
        let letter = GREEK.get(i).copied().unwrap_or("τ");
        let n = self.kp_counter;
        self.kp_counter += 1;
        format!("{letter}${n}$")
    }

    /// `{ type Λ$[tparams] = body }#Λ$`.
    fn kp_lambda(&mut self, span: Span, tparams: Vec<Tree>, body: Tree) -> Tree {
        let name = lambda_name(self.file_index, self.kp_lambdas);
        self.kp_lambdas += 1;
        let decl = self.alloc(
            span,
            TreeKind::TypeDef {
                mods: Modifiers::default(),
                name: name.clone(),
                tparams,
                rhs: Box::new(body),
                // The plugin writes `>: Nothing <: Any` on the parameters it
                // invents. That is what an absent bound already means here, so
                // there is nothing to write.
                lo: None,
                hi: None,
                views: vec![],
                ctx_bounds: vec![],
            },
        );
        let refined = self.alloc(
            span,
            TreeKind::CompoundTypeTree {
                parents: vec![],
                refinements: vec![decl],
            },
        );
        self.alloc(
            span,
            TreeKind::SelectFromTypeTree {
                qual: Box::new(refined),
                name,
                hash: true,
            },
        )
    }

    /// One type parameter of the lambda, from the tree written in the lambda's
    /// parameter position.
    ///
    /// The plugin's rule is that the head name of the tree becomes the
    /// parameter and its arguments become that parameter's own parameters,
    /// whatever they are: `A[_]` is `A[_]`, and `Box[Int]` -- which nobody
    /// means to write -- becomes a parameter `Box` taking a parameter `Int`,
    /// which is what `-Xprint:kind-projector` shows for it. `None` is for the
    /// shapes it does not touch at all; the caller then leaves the lambda
    /// standing and the typer reports `not found: type λ`, exactly as nsc with
    /// the plugin does.
    fn kp_tparam(&mut self, t: &Tree) -> Option<Tree> {
        let (flags, name, inner) = match &t.kind {
            // A plain name, and the backquoted variance spelling `` `+a` `` --
            // backquotes are the only way to write it, because an unquoted
            // `+a` lexes as two tokens.
            TreeKind::Ident { name } => {
                let (flags, base) = split_variance(name);
                if base.is_empty() {
                    return None;
                }
                (flags, base.to_string(), Vec::new())
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                let head = match &tpt.kind {
                    TreeKind::Ident { name } => name.clone(),
                    _ => return None,
                };
                // `Lambda[(-[A], +[B]) => ...]`: the variance written as an
                // application of `-` / `+` rather than in a backquoted name.
                if (head == "+" || head == "-") && args.len() == 1 {
                    let mut inner = self.kp_tparam(&args[0])?;
                    if let TreeKind::TypeDef { mods, .. } = &mut inner.kind {
                        mods.flags = mods.flags.with(if head == "+" {
                            Flags::COVARIANT
                        } else {
                            Flags::CONTRAVARIANT
                        });
                    }
                    return Some(inner);
                }
                let mut tps = Vec::new();
                for a in args {
                    match &a.kind {
                        // `_` already arrives as the type-parameter tree a
                        // `type A[_]` clause would have.
                        TreeKind::TypeDef { .. } => tps.push(a.clone()),
                        _ => tps.push(self.kp_tparam(a)?),
                    }
                }
                let (flags, base) = split_variance(&head);
                if base.is_empty() {
                    return None;
                }
                (flags, base.to_string(), tps)
            }
            _ => return None,
        };
        let rhs = self.empty(t.span);
        Some(self.alloc(
            t.span,
            TreeKind::TypeDef {
                mods: Modifiers {
                    flags,
                    ..Modifiers::default()
                },
                name,
                tparams: inner,
                rhs: Box::new(rhs),
                lo: None,
                hi: None,
                views: vec![],
                ctx_bounds: vec![],
            },
        ))
    }

    /// Rewrite a type application the type parser has just finished, when
    /// `-Ykind-projector` is on and it is one of the plugin's two forms.
    /// Everything else is returned unchanged.
    pub(super) fn kp_type(&mut self, t: Tree) -> Tree {
        if !self.kp() {
            return t;
        }
        match self.kp_lambda_app(&t) {
            Some(rewritten) => rewritten,
            None => self.kp_placeholders(t),
        }
    }

    /// A parenthesised type that turned out to be a tuple and not a function's
    /// parameter list. `(A0, *)` is `Tuple2[A0, *]` to the plugin, but this
    /// parser leaves the marker `<tuple>` on it until the typer, because
    /// `(A0, *) => R` has to stay two parameters.
    pub(super) fn kp_tuple(&mut self, mut t: Tree) -> Tree {
        if !self.kp() {
            return t;
        }
        let arity = match &t.kind {
            TreeKind::AppliedTypeTree { tpt, args }
                if matches!(&tpt.kind, TreeKind::Ident { name } if name == "<tuple>")
                    && args.iter().any(|a| placeholder_arg(a).is_some()) =>
            {
                args.len()
            }
            _ => return t,
        };
        if let TreeKind::AppliedTypeTree { tpt, .. } = &mut t.kind {
            tpt.kind = TreeKind::Ident {
                name: format!("Tuple{arity}"),
            };
        }
        self.kp_placeholders(t)
    }

    /// `λ[…]` / `Lambda[…]`.
    ///
    /// A shape the plugin does not recognise is left exactly as written, which
    /// is what the plugin itself does: `scalac -Xplugin:kind-projector`
    /// reports `not found: type λ` for `λ[Int]` and for `λ[α => F[α], β]`,
    /// because its rewriter passes them through untouched. A diagnostic of our
    /// own here would be one nsc does not have.
    fn kp_lambda_app(&mut self, t: &Tree) -> Option<Tree> {
        let (tpt, args) = match &t.kind {
            TreeKind::AppliedTypeTree { tpt, args } => (tpt, args),
            _ => return None,
        };
        match &tpt.kind {
            TreeKind::Ident { name } if name == "λ" || name == "Lambda" => {}
            _ => return None,
        }
        if args.len() != 1 {
            return None;
        }
        // The parser has already folded `a => B` into `Function1[a, B]` and
        // `(a, b) => B` into `Function2[a, b, B]`.
        let (params, body) = match &args[0].kind {
            TreeKind::AppliedTypeTree { tpt, args: fargs }
                if function_arity(&tpt.kind).is_some_and(|n| n >= 1 && n + 1 == fargs.len()) =>
            {
                let (ps, b) = fargs.split_at(fargs.len() - 1);
                (ps.to_vec(), b[0].clone())
            }
            _ => return None,
        };
        let mut tparams = Vec::with_capacity(params.len());
        for p in &params {
            tparams.push(self.kp_tparam(p)?);
        }
        Some(self.kp_lambda(t.span, tparams, body))
    }

    /// `F[…, *, …]`.
    fn kp_placeholders(&mut self, t: Tree) -> Tree {
        let (tpt, args) = match &t.kind {
            TreeKind::AppliedTypeTree { tpt, args } => (tpt, args),
            _ => return t,
        };
        // `<tuple>` is still a parameter-list marker at this point; `kp_tuple`
        // handles it once the parser knows there is no `=>` after it.
        if matches!(&tpt.kind, TreeKind::Ident { name } if name == "<tuple>") {
            return t;
        }
        if !args.iter().any(|a| placeholder_arg(a).is_some()) {
            return t;
        }
        let tpt = tpt.clone();
        let args = args.clone();
        let mut tparams = Vec::new();
        let mut rewritten = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            match placeholder_arg(a) {
                Some((flags, inner)) => {
                    let name = self.kp_fresh(i);
                    let inner = inner.to_vec();
                    let rhs = self.empty(a.span);
                    tparams.push(self.alloc(
                        a.span,
                        TreeKind::TypeDef {
                            mods: Modifiers {
                                flags,
                                ..Modifiers::default()
                            },
                            name: name.clone(),
                            tparams: inner,
                            rhs: Box::new(rhs),
                            lo: None,
                            hi: None,
                            views: vec![],
                            ctx_bounds: vec![],
                        },
                    ));
                    rewritten.push(self.alloc(a.span, TreeKind::Ident { name }));
                }
                None => rewritten.push(a.clone()),
            }
        }
        let span = t.span;
        let body = self.alloc(
            span,
            TreeKind::AppliedTypeTree {
                tpt,
                args: rewritten,
            },
        );
        self.kp_lambda(span, tparams, body)
    }
}

/// `+α` / `-α` written as one (backquoted) identifier.
fn split_variance(name: &str) -> (Flags, &str) {
    if let Some(rest) = name.strip_prefix('+') {
        (Flags::EMPTY.with(Flags::COVARIANT), rest)
    } else if let Some(rest) = name.strip_prefix('-') {
        (Flags::EMPTY.with(Flags::CONTRAVARIANT), rest)
    } else {
        (Flags::EMPTY, name)
    }
}

/// `FunctionN` -> `N`. The type parser folds every `=>` into one of these.
fn function_arity(kind: &TreeKind) -> Option<usize> {
    match kind {
        TreeKind::Ident { name } => name.strip_prefix("Function")?.parse::<usize>().ok(),
        _ => None,
    }
}
