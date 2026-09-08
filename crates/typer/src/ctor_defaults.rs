//! Default-argument getters for a *primary constructor*.
//!
//! An ordinary method's defaults get `name$default$n` getters on the method's
//! own owner (`Typer::synthesize_default_getters`). A constructor's cannot go
//! there: `new C(1)` has no receiver to call them on, so nsc puts them on
//! `C`'s **companion module** instead, under two names:
//!
//! ```text
//! C$.$lessinit$greater$default$n()   // for `new C(...)`
//! C$.apply$default$n()               // for a case class's synthetic `apply`
//! ```
//!
//! scala-rs fills an omitted constructor argument by splicing the stored
//! default expression into the call (`Typer::type_default_rhs_here`), which is
//! enough inside one compilation run and invisible from outside it. A
//! *separately compiled* caller emits the getter call, and slick's
//!
//! ```scala
//! case class Length(length: Int, varying: Boolean = true) extends ColumnOption[Nothing]
//! ```
//!
//! is reached from client code as `O.Length(64)`, which real scalac compiles
//! to `Length$.apply$default$2()` followed by `Length$.apply(64, z)` --
//! `NoSuchMethodError` against a slick that scala-rs built.
//!
//! So the getters are declared here as members of the companion **module
//! class**, carrying the default's typed body; `Gen::emit_default_getters`
//! picks up every `$default$` member of a class it emits and writes the body
//! out, so no codegen of its own is needed.
//!
//! Two restrictions, both matching what the rest of the compiler already does:
//!
//! * only a class that *has* a companion module gets them. nsc synthesizes one
//!   for a plain `class C(a: Int, b: Int = 7)` with no companion; doing that
//!   here would add classfiles, so `new C(1)` from a separately compiled
//!   caller stays unsupported (see `docs/not-implemented.md`).
//! * a constructor default may not name an earlier constructor parameter --
//!   `Typer::record_default_scope` drops the class's member scope for exactly
//!   that reason -- so the body is always closed over the getter's own
//!   parameter list, which is the preceding *clauses* only, as in nsc.

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::ast::Type;
use scala_rs_parser::{Flags, SymbolId};
use scala_rs_span::Span;

impl Typer {
    /// Declare `$lessinit$greater$default$n` (and, for a case class with a
    /// synthetic `apply`, `apply$default$n`) on `class_id`'s companion module
    /// class, one pair per defaulted primary-constructor parameter.
    ///
    /// Idempotent: the constructor parameters are typed by both the header
    /// pass and the signature pass, so this runs more than once per class.
    pub(crate) fn synthesize_ctor_default_getters(
        &mut self,
        class_id: SymbolId,
        paramss_ids: &[Vec<SymbolId>],
    ) {
        self.synthesize_ctor_default_getters_of(class_id, paramss_ids, true)
    }

    /// The same, for one *secondary* constructor's parameter list.
    ///
    /// nsc names a secondary's getters `$lessinit$greater$default$n` too, with
    /// `n` counted over that constructor's own flattened parameters -- `javap`
    /// on scalac 2.13.16's output for
    /// `class Chain(v: String) { def this(n: Int, sep: String = "-") = … }`
    /// shows one `$lessinit$greater$default$2`. A secondary owes **no**
    /// `apply$default$n`: a case class's synthetic `apply` mirrors the
    /// *primary* alone.
    ///
    /// Only one constructor of a class may define defaults at all
    /// (`check_ctor_default_overloads`, which is nsc's own rule), so the two
    /// callers can never be asked for the same getter name with different
    /// bodies.
    pub(crate) fn synthesize_secondary_ctor_default_getters(
        &mut self,
        class_id: SymbolId,
        ctor: SymbolId,
    ) {
        if ctor.is_none() {
            return;
        }
        let paramss = self.st.get(ctor).paramss.clone();
        self.synthesize_ctor_default_getters_of(class_id, &paramss, false)
    }

    fn synthesize_ctor_default_getters_of(
        &mut self,
        class_id: SymbolId,
        paramss_ids: &[Vec<SymbolId>],
        primary: bool,
    ) {
        if class_id.is_none() {
            return;
        }
        let Some(module) = self.st.companion_module(class_id) else {
            return;
        };
        let owner = self.st.module_class_of(module);
        if owner.is_none() {
            return;
        }
        // Only a companion that carries the synthetic `apply` owes an
        // `apply$default$n`: a user-written `apply` suppresses the synthetic
        // one, and its own defaults are ordinary method defaults.
        let has_case_apply = primary
            && self.st.get(owner).members.iter().any(|&m| {
                self.st.get(m).name == "apply" && self.st.get(m).flags.contains(Flags::CASE)
            });
        let tparams = self.st.get(class_id).tparams.clone();
        let flat: Vec<SymbolId> = paramss_ids.iter().flatten().copied().collect();
        for (i, pid) in flat.iter().enumerate() {
            let s = self.st.get(*pid);
            if !s.flags.contains(Flags::DEFAULTPARAM) || s.default_rhs.is_none() {
                continue;
            }
            let n = i + 1;
            // nsc's getter takes the parameter clauses that *precede* the
            // defaulted one's clause; a same-clause reference is rejected
            // outright, so nothing else can be in scope for the body.
            let cut = crate::check::clause_start_of(paramss_ids, i);
            let preceding: Vec<SymbolId> = flat[..cut].to_vec();
            let preceding_tys: Vec<Type> = preceding
                .iter()
                .map(|id| self.st.get(*id).ty.clone())
                .collect();
            let declared = self.st.get(*pid).ty.clone();
            // nsc leaves the getter's result type to be *inferred* whenever
            // the parameter's type names one of the class's type parameters,
            // and slick relies on it: `case class Comprehension[+Fetch <:
            // Option[Node]](…, fetch: Fetch = None, …)` has no `None` that
            // conforms to `Fetch`, and nsc's
            // `$lessinit$greater$default$9` is declared `scala.None$`.
            // Only the call site knows what `Fetch` is; the getter answers
            // with the type of the expression it holds.
            let mut used = Vec::new();
            crate::check::collect_tparams(&declared, &mut used);
            let infer_ret = used.iter().any(|u| tparams.contains(u));
            let ret = if infer_ret {
                Type::NoType
            } else {
                declared.clone()
            };
            let mut names = vec![format!("$lessinit$greater$default${n}")];
            if has_case_apply {
                names.push(format!("apply$default${n}"));
            }
            for gname in names {
                if self
                    .st
                    .get(owner)
                    .members
                    .iter()
                    .any(|&id| self.st.get(id).name == gname)
                {
                    continue;
                }
                let gid = self
                    .st
                    .alloc(&gname, owner, SymKind::Method, Flags::SYNTHETIC, "");
                // With nothing preceding it the getter is *nullary* -- nsc's
                // `def apply$default$1: Int`, a `NullaryMethodType`, not a
                // method over an empty list. The two are different types to a
                // reader, and pickled as `apply$default$1()` real scalac warns
                // "Auto-application to `()` is deprecated" at every call that
                // omits the argument. See `check_member::synthesize_default_
                // getters`, which says the same for `copy$default$n`.
                self.st.get_mut(gid).ty = Type::Method {
                    paramss: if preceding_tys.is_empty() {
                        Vec::new()
                    } else {
                        vec![preceding_tys.clone()]
                    },
                    ret: Box::new(ret.clone()),
                };
                self.st.get_mut(gid).params = preceding.clone();
                self.st.get_mut(gid).paramss = if preceding.is_empty() {
                    Vec::new()
                } else {
                    vec![preceding.clone()]
                };
                // A generic class's default may name the class's own type
                // parameters (`class C[A](xs: List[A] = Nil)`); nsc's getter
                // repeats them, and they are erased away by codegen.
                self.st.get_mut(gid).tparams = tparams.clone();
                // The body belongs to the *definition's* scope, not to
                // whatever is current here, and it may name a member of a unit
                // the command line has not reached yet -- so it is typed with
                // the rest of the deferred defaults. Only the *getter* keeps
                // the typed tree: the parameter's own `default_rhs` stays the
                // namer's untyped one, which is what a call site splices and
                // re-types in its own unit
                // (`Typer::type_default_rhs_here`).
                self.defer_ctor_default_getter_rhs(*pid, gid, &ret, &tparams, &preceding);
            }
        }
    }

    /// nsc: `in class Two, multiple overloaded alternatives of constructor Two
    /// define default arguments.`
    ///
    /// The getters above are named after the *parameter position* and nothing
    /// else, so two constructors that both define defaults want the same
    /// `$lessinit$greater$default$2` with different bodies. nsc forbids that
    /// outright for any overloaded method, constructors included, which is why
    /// the name is enough for it too. Checked here rather than left to "first
    /// declaration wins": the second constructor's caller would silently get
    /// the first's default, and no error count would show it.
    ///
    /// Verified against scalac 2.13.16, which rejects
    /// `class Two(v: String) { def this(n: Int, sep: String = "-") = …
    ///                         def this(f: Boolean, tag: String = "t") = … }`
    /// at the class's own position.
    pub(crate) fn check_ctor_default_overloads(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let ctors: Vec<SymbolId> = self
            .st
            .get(class_id)
            .members
            .iter()
            .copied()
            .filter(|&m| self.st.get(m).name == "<init>")
            .collect();
        let mut with_defaults = 0usize;
        for c in ctors {
            let params: Vec<SymbolId> = if self.st.get(c).paramss.is_empty() {
                self.st.get(c).params.clone()
            } else {
                self.st.get(c).paramss.iter().flatten().copied().collect()
            };
            if params
                .iter()
                .any(|&p| self.st.get(p).flags.contains(Flags::DEFAULTPARAM))
            {
                with_defaults += 1;
            }
        }
        if with_defaults > 1 {
            let name = self.st.get(class_id).name.clone();
            let kind = if self.st.get(class_id).flags.contains(Flags::TRAIT) {
                "trait"
            } else {
                "class"
            };
            self.error(
                span,
                format!(
                    "in {kind} {name}, multiple overloaded alternatives of constructor {name} define default arguments."
                ),
            );
        }
    }
}
