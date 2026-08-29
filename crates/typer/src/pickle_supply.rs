//! Complete standard-library members from their `ScalaSignature` pickles, on
//! demand, when the hand-written prelude does not have them.
//!
//! `prelude.rs` declares library members by hand. That does not scale to a
//! 2.13-compatible surface, so this module fills the gaps: when member
//! resolution on a `scala.*` receiver fails outright, the receiver's pickle is
//! read (and its parents', through `scala-rs-pickle`'s `SigCache`) and the
//! missing member is installed on the receiver's class symbol.
//!
//! Three rules keep it honest:
//!
//! 1. **The prelude always wins.** This runs only when `lookup_member` found
//!    *nothing*, so a hand-written declaration is never shadowed or replaced.
//! 2. **A member we cannot express is not supplied.** If the pickled type does
//!    not map onto `scala_rs_parser::Type`, or if we cannot pin down the erased
//!    JVM descriptor to call it with, the member is skipped and the user gets
//!    the usual "is not a member" error. A wrong type would be worse than none.
//! 3. **Nothing is read ahead of time.** One classfile per (receiver, name)
//!    miss, cached.

use std::collections::{HashMap, HashSet};

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_pickle::read::pflags;
use scala_rs_pickle::sym::{MemberKind, SigCache, SigType};
use scala_rs_pickle::ClassSource;

use crate::javaclass::{parse_java_classfile, BinaryIndex, JavaClass};
use crate::symbol::{SymKind, SymbolTable};

/// `SCALA_RS_PICKLE_DEBUG=1` traces why a member was or was not supplied.
/// Completion is silent otherwise: a member it declines to supply surfaces as
/// the typer's ordinary "is not a member".
pub(crate) fn trace(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("SCALA_RS_PICKLE_DEBUG").is_some() {
        eprintln!("[pickle] {args}");
    }
}

/// One JVM method declaration matching a pickled member.
struct ErasedDecl {
    /// The erased descriptor to call it with.
    desc: String,
    /// JVM internal name of the class file that declares it.
    declared_in: String,
    /// Whether that class file is an interface, and so whether the call is
    /// `invokeinterface` or `invokevirtual`.
    declared_by_interface: bool,
    /// True when the search reached `declared_in` only through a *pickled*
    /// parent -- a hop the JVM cannot make, because the class file that named
    /// it declares no parents at all (`api/JavaUniverse` has `interfaces: 0`
    /// even though the trait extends the abstract class `Universe`).
    ///
    /// The call then cannot name the receiver's class: resolution would look
    /// only where the bytecode leads and find nothing. It has to name
    /// `declared_in`, with the receiver cast to it first.
    off_the_bytecode_path: bool,
}

const ACC_STATIC: u16 = 0x0008;
const ACC_BRIDGE: u16 = 0x0040;
const ACC_SYNTHETIC: u16 = 0x1000;

/// A `ClassSource` view of the typer's `BinaryIndex`.
struct BinSource<'a>(&'a mut BinaryIndex);

impl ClassSource for BinSource<'_> {
    fn class_bytes(&mut self, internal_name: &str) -> Option<Vec<u8>> {
        self.0.find_class(internal_name).ok().flatten()
    }
}

#[derive(Default)]
pub struct PickleSupply {
    sigs: SigCache,
    /// `(receiver class, member name)` pairs already attempted, so a miss
    /// costs one lookup, not one per mention.
    tried: HashSet<(u32, String)>,
    /// Parsed classfiles, for erased descriptors.
    classes: HashMap<String, Option<JavaClass>>,
    /// Library classes stubbed into the symbol table by `ensure_class`, keyed
    /// by JVM internal name, so a second mention reuses the same symbol.
    stubs: HashMap<String, SymbolId>,
    /// Classes whose pickled parents have already been attached.
    parented: HashSet<u32>,
    /// Dotted names `conv` needed and could not turn into a symbol, since the
    /// last `take_unresolved_refs`. The typer uses them to decide which
    /// classfiles to load before trying again (see `Check::pickled_alias_type`).
    unresolved_refs: Vec<String>,
    /// What `this.type` means while a member of one class is being installed:
    /// the receiver applied to its own type parameters. `Growable.addOne` and
    /// `+=` / `++=` all return `this.type`, and for a
    /// `Builder[Int, List[Int]]` receiver that is a `Builder`, not the
    /// `Growable` the erased signature names.
    self_ty: Option<Type>,
    /// Classes read from a `-cp` jar/directory whose shape has been taken from
    /// their `ScalaSignature` rather than from the JVM generic signature (see
    /// [`PickleSupply::adopt_binary_class`]). Also the gate that lets
    /// `complete_named` serve a class outside `scala.*`.
    adopted: HashSet<u32>,
}

/// One `type T[...] = U` recovered from a package object's pickle.
///
/// Package-object aliases exist *only* in the pickle: `<pkg>/package$.class`
/// declares no member for them, so reading the classfile leaves them behind.
#[derive(Clone, Debug)]
pub struct PickledAlias {
    pub name: String,
    pub tparams: Vec<PickledTParam>,
    /// The right-hand side, still in the alias's own vocabulary.
    pub rhs: SigType,
}

/// A type parameter of a pickled alias. `arity` is its own kind arity, so
/// `type Ref[F[_], A]` yields `[("F", 1), ("A", 0)]`.
#[derive(Clone, Debug)]
pub struct PickledTParam {
    pub name: String,
    pub arity: usize,
}

impl PickleSupply {
    pub fn new() -> Self {
        PickleSupply::default()
    }

    /// Try to install `name` on `class_sym` from the library pickles.
    /// Returns true if at least one member was installed.
    /// Try to install `name` for a receiver whose class symbol is `class_sym`.
    ///
    /// Returns the symbols it installed, and on which owner: a member found on
    /// the companion object lands on the companion's module class, not on
    /// `class_sym`, so the caller has to look there too.
    pub fn complete(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Vec<SymbolId> {
        let mut out = self.complete_on(st, bin, class_sym, name);
        // `Iterator.from(1)`: the prelude has the trait but no companion, so
        // the receiver resolved to the class. The member lives on the
        // companion object, which is where it has to be installed -- putting
        // it on the trait would emit an invokevirtual against a method that is
        // not there. codegen already loads `X$.MODULE$` when a method's owner
        // is a module class, so this comes out right.
        //
        // The companion is consulted even when the class itself supplied
        // something, and the two results are unioned. Gating it on "the class
        // supplied nothing" made the answer depend on unrelated global state:
        // `scala.math.BigDecimal` declares an instance `apply(MathContext)`,
        // whose parameter `conv` can only map once *some other* code has
        // pulled `java.math.MathContext` into the symbol table. So
        // `BigDecimal(2)` resolved against the companion's seven `apply`
        // overloads on its own, but against that single instance `apply` in a
        // unit that had already mentioned `java.math.BigDecimal` -- the same
        // program compiled or not depending on statement order. Completion is
        // additive by contract; a class-side hit must not hide the
        // companion's.
        if !class_sym.is_none() && st.get(class_sym).kind == SymKind::Class {
            let internal = st.get(class_sym).jvm_name.clone();
            if internal.starts_with("scala/") {
                let full = internal.replace('/', ".");
                if let Some(m) = self.ensure_class(st, bin, &full, true) {
                    if m != class_sym {
                        out.extend(self.complete_on(st, bin, m, name));
                    }
                }
            }
        }
        out
    }

    /// The `type` aliases a package object declares, read from its pickle.
    ///
    /// `po_full` is the package object's dotted name (`scala.package`).
    /// `Err` says the pickle is not there or does not parse -- the caller then
    /// supplies nothing, rather than guessing.
    pub fn package_object_aliases(
        &mut self,
        bin: &mut BinaryIndex,
        po_full: &str,
    ) -> Result<Vec<PickledAlias>, String> {
        let sig = {
            let mut src = BinSource(bin);
            self.sigs
                .class_sig(&mut src, po_full, true)
                .map_err(|e| e.to_string())?
        };
        let mut out = Vec::new();
        for m in &sig.members {
            if m.kind != MemberKind::TypeAlias || !m.is_public_api() {
                continue;
            }
            let (tps, rhs) = match &m.ty {
                SigType::Poly { tparams, result } => (tparams.as_slice(), (**result).clone()),
                other => (&[][..], other.clone()),
            };
            out.push(PickledAlias {
                name: m.name.clone(),
                tparams: tps
                    .iter()
                    .map(|tp| PickledTParam {
                        name: tp.name.clone(),
                        arity: tparam_arity(tp),
                    })
                    .collect(),
                rhs,
            });
        }
        Ok(out)
    }

    /// Map a pickled type onto the typer's, in `scope`'s vocabulary.
    ///
    /// `None` means the type could not be expressed; the names that made it
    /// fail are then available from `take_unresolved_refs`, so the caller can
    /// load those classfiles and try again.
    pub fn convert_pickled_type(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
    ) -> Option<Type> {
        self.unresolved_refs.clear();
        self.conv(st, bin, scope, t)
    }

    /// Dotted names the last `convert_pickled_type` could not resolve.
    pub fn take_unresolved_refs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unresolved_refs)
    }

    /// Re-read a class just loaded from a `-cp` classfile from its own pickle.
    ///
    /// `install_java_class_in` builds the symbol out of the JVM *generic
    /// signature*, and that format cannot write a higher kind or a higher-kinded
    /// application. `trait Monad[F[_]]` arrives as `Monad[F]` with `F` a proper
    /// type, and `def pure[A](a: A): F[A]` as `(A)F` — so `Monad[F]` is a kind
    /// error at every use site and `F.pure(v)` is `found: F, required: F[R]`.
    /// scalac reads none of that from the signature: it reads the
    /// `ScalaSignature` pickle, which has the real one.
    ///
    /// So does this. The class keeps the symbol it already has (its parents,
    /// flags and fields all come from the classfile) and gains:
    ///
    /// * its pickled type parameters' *kinds*, and
    /// * one pickled signature per member the pickle declares, replacing the
    ///   JVM-signature member of the same name **only when the pickled one
    ///   could be expressed**. A member the pickle path declines is left
    ///   exactly as the classfile reader installed it, so this can add
    ///   precision but never take a member away.
    ///
    /// Only classes outside `scala.*` / `java.*`: the standard library reaches
    /// the typer through the hand-written prelude plus `complete`, and that
    /// path is the one the prelude's shapes are validated against.
    ///
    /// Returns true if the class was adopted (so `complete_named` may serve it
    /// from now on).
    pub fn adopt_binary_class(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
    ) -> bool {
        if class_sym.is_none() || !st.get(class_sym).is_class_like() {
            return false;
        }
        let internal = st.get(class_sym).jvm_name.clone();
        if internal.is_empty()
            || internal.starts_with("scala/")
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
            || internal.contains("$anon")
        {
            return false;
        }
        if self.adopted.contains(&class_sym.0) {
            return true;
        }
        let is_module = st.get(class_sym).kind == SymKind::ModuleClass;
        let Some(full) = self.pickled_full_name(bin, &internal, is_module) else {
            trace(format_args!("{internal}: no pickle to adopt"));
            return false;
        };
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, &full, is_module)
        }) else {
            return false;
        };
        // Marked adopted before any member work: `complete_named` refuses a
        // class it does not know, and installing a member re-enters it (a
        // `$default$n` getter).
        self.adopted.insert(class_sym.0);
        trace(format_args!("adopting {full} (module={is_module})"));
        adopt_tparam_kinds(st, class_sym, &sig);

        // Member names in declaration order, deduplicated: `complete_named`
        // installs every overload of a name at once.
        let mut names: Vec<String> = Vec::new();
        for m in &sig.members {
            if m.kind != MemberKind::Def || !m.is_public_api() {
                continue;
            }
            let src_name = scala_rs_pickle::names::decode_method_name(&m.name);
            if src_name.is_empty() || src_name == "<init>" || src_name.contains('$') {
                continue;
            }
            if !names.contains(&src_name) {
                names.push(src_name);
            }
        }
        for name in names {
            // What the classfile reader put there, so it can be dropped once
            // the pickle has supplied something better.
            let stale: Vec<SymbolId> = st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| {
                    let s = st.get(m);
                    s.kind == SymKind::Method && s.name == name
                })
                .collect();
            let installed = self.complete_named(st, bin, class_sym, &name, false);
            if installed.is_empty() {
                continue;
            }
            st.get_mut(class_sym).members.retain(|m| !stale.contains(m));
        }
        true
    }

    /// The dotted name whose pickle describes `internal`, if there is one.
    ///
    /// A pickle names a nested class `Outer.Inner` while its classfile is
    /// `Outer$Inner`, so both spellings are tried (the `$` form first, since a
    /// `$` in a top-level name is part of that name).
    fn pickled_full_name(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        is_module: bool,
    ) -> Option<String> {
        let full = internal.trim_end_matches('$').replace('/', ".");
        if self.has_pickle(bin, &full, is_module) {
            return Some(full);
        }
        let dotted = full.replace('$', ".");
        if dotted != full && self.has_pickle(bin, &dotted, is_module) {
            return Some(dotted);
        }
        None
    }

    fn complete_on(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> Vec<SymbolId> {
        self.complete_named(st, bin, class_sym, name, false)
    }

    /// `synthetic_ok` is set only when fetching a `$default$` getter, which is
    /// synthetic by construction and would otherwise be filtered out.
    fn complete_named(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
        synthetic_ok: bool,
    ) -> Vec<SymbolId> {
        if class_sym.is_none() || name.is_empty() {
            return Vec::new();
        }
        if !self.tried.insert((class_sym.0, name.to_string())) {
            return st
                .get(class_sym)
                .members
                .iter()
                .copied()
                .filter(|&m| st.get(m).name == name)
                .collect();
        }
        // nsc keeps operator names encoded all the way through: `SetOps`
        // pickles `&` as `$amp`, and the classfile declares `$amp` too. So the
        // encoded name is what both the pickle lookup and the descriptor
        // search use, while the symbol we install keeps the source name.
        let jvm_member = scala_rs_pickle::names::encode_method_name(name);
        if !jvm_member
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        {
            trace(format_args!("{name}: not encodable as a JVM method name"));
            return Vec::new();
        }
        let sym = st.get(class_sym);
        if !sym.is_class_like() {
            return Vec::new();
        }
        let internal = sym.jvm_name.clone();
        // Scoped to the standard library, plus any class `adopt_binary_class`
        // has taken over: those two are the pickles the typer reads. A plain
        // Java classfile on `-cp` has no pickle at all and keeps its own path
        // in through `install_java_class`.
        if !internal.starts_with("scala/") && !self.adopted.contains(&class_sym.0) {
            return Vec::new();
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        // A pickle names a nested class `Outer.Inner`; its JVM name is
        // `Outer$Inner`. Without undoing that, `Constants$ConstantExtractor`
        // -- the receiver of `u.Constant(1)` -- has no pickle and so no
        // members at all. The `$` form is tried first, because a `$` in a
        // top-level name is part of that name.
        let full = internal.trim_end_matches('$').replace('/', ".");
        let full = if self.has_pickle(bin, &full, is_module) {
            full
        } else {
            let dotted = full.replace('$', ".");
            if dotted != full && self.has_pickle(bin, &dotted, is_module) {
                dotted
            } else {
                full
            }
        };

        self.attach_parents(st, bin, class_sym, &full, is_module);

        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, is_module, &jvm_member)
        };
        if hits.is_empty() {
            trace(format_args!("{full}#{name}: not found in any pickle"));
            return Vec::new();
        }

        // The receiver's own type parameters are the vocabulary the looked-up
        // types are already expressed in.
        let mut class_scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            class_scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        // Nested completion (a `$default$n` getter) re-enters here, so the
        // outer receiver's meaning of `this.type` is saved and restored.
        let saved_self = self.self_ty.replace(Type::Class {
            sym: class_sym,
            args: st
                .get(class_sym)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        });

        let mut installed: Vec<SymbolId> = Vec::new();
        let mut seen_shapes: HashSet<String> = HashSet::new();
        // Whether an overload taking a function parameter is already in.
        let mut took_function = false;
        for hit in hits {
            let m = hit.member;
            if m.kind != MemberKind::Def {
                continue;
            }
            if !m.is_public_api() && !(synthetic_ok && is_default_getter(&m.name)) {
                continue;
            }
            let Some(shape) = read_shape(&m.ty) else {
                trace(format_args!(
                    "{internal}#{name}: unreadable signature shape"
                ));
                continue;
            };
            let Some(shape) = pin_undetermined_tparams(shape) else {
                trace(format_args!(
                    "{internal}#{name}: type parameter appears only in an implicit \
                     clause and has no lower bound to pin it to"
                ));
                continue;
            };
            if let Some(id) = self.install(
                st,
                bin,
                class_sym,
                &internal,
                name,
                &jvm_member,
                &shape,
                &class_scope,
                &mut seen_shapes,
                &mut took_function,
            ) {
                installed.push(id);
            }
        }
        self.self_ty = saved_self;
        trace(format_args!(
            "{full}#{name}: supplied {} overload(s)",
            installed.len()
        ));
        installed
    }

    #[allow(clippy::too_many_arguments)]
    fn install(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        name: &str,
        jvm_member: &str,
        shape: &Shape,
        class_scope: &HashMap<String, Type>,
        seen_shapes: &mut HashSet<String>,
        took_function: &mut bool,
    ) -> Option<SymbolId> {
        // Allocated ownerless, so a failure leaves nothing behind:
        // `SymbolTable::alloc` pushes into the owner's member list.
        let m = st.alloc(name, SymbolId::NONE, SymKind::Method, Flags::EMPTY, "");

        let mut scope = class_scope.clone();
        let mut tparams = Vec::new();
        for tp in &shape.tparams {
            let id = st.alloc(&tp.name, m, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(id).ty = Type::TypeParam(id);
            set_tparam_arity(st, id, tp.arity);
            scope.insert(tp.name.clone(), Type::TypeParam(id));
            tparams.push(id);
        }
        // Bounds are resolved after every parameter is in scope (`A <: B`).
        // The lower bound matters as much as the upper one: `[B >: A]` is what
        // lets the typer solve `B` from the receiver for `xs.reduceOption`.
        for (tp, id) in shape.tparams.iter().zip(tparams.iter().copied()) {
            if let Some(hi) = &tp.hi {
                if let Some(t) = self.conv(st, bin, &scope, hi) {
                    st.get_mut(id).bound_hi = Some(t);
                }
            }
            if let Some(lo) = &tp.lo {
                if !matches!(lo, SigType::Ref { sym, .. } if sym == "scala.Nothing") {
                    if let Some(t) = self.conv(st, bin, &scope, lo) {
                        st.get_mut(id).bound_lo = Some(t);
                    }
                }
            }
        }

        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_sym: Vec<Vec<SymbolId>> = Vec::new();
        // 1-based positions of parameters the caller may omit.
        let mut default_slots: Vec<usize> = Vec::new();
        for clause in &shape.clauses {
            let mut tys = Vec::new();
            let mut syms = Vec::new();
            for p in &clause.params {
                let Some(mut t) = self.conv(st, bin, &scope, &p.ty) else {
                    trace(format_args!(
                        "{internal}#{name}: parameter {} has an unmappable type {:?}",
                        p.name, p.ty
                    ));
                    return None;
                };
                if p.by_name && !matches!(t, Type::ByName(_)) {
                    t = Type::ByName(Box::new(t));
                }
                let mut flags = if clause.implicit {
                    Flags::PARAM.with(Flags::IMPLICIT)
                } else {
                    Flags::PARAM
                };
                if p.flags & pflags::DEFAULTPARAM != 0 {
                    flags = flags.with(Flags::DEFAULTPARAM);
                    default_slots
                        .push(paramss_ty.iter().map(|c| c.len()).sum::<usize>() + tys.len() + 1);
                }
                let ps = st.alloc(&p.name, m, SymKind::Term, flags, "");
                st.get_mut(ps).ty = t.clone();
                tys.push(t);
                syms.push(ps);
            }
            paramss_ty.push(tys);
            paramss_sym.push(syms);
        }
        let Some(ret) = self.conv(st, bin, &scope, &shape.ret) else {
            trace(format_args!(
                "{internal}#{name}: unmappable result type {:?}",
                shape.ret
            ));
            return None;
        };

        // The erased descriptor comes from the classfile itself rather than
        // from re-deriving scalac's erasure: the bytes are the truth, and a
        // descriptor we merely guessed would fail to link. Resolved now that
        // the parameters are known, so same-arity overloads
        // (`Iterator.from(Int)` vs `from(IterableOnce)`) can be told apart.
        let want: Vec<Option<String>> = paramss_ty
            .iter()
            .flatten()
            .map(|t| erased_param_desc(st, t))
            .collect();
        // One declaration per erased parameter list, the first one
        // linearization offers, i.e. the most derived. Two declarations that
        // erase alike are the same JVM method seen through different parents,
        // and where they are genuinely overloaded on the *result*
        // (`IterableOps.map[B]: Iterable[B]` vs `MapOps.map[K2,V2]:
        // Map[K2,V2]`) scalac picks by expected type, which the typer cannot
        // do -- supplying both would make every call ambiguous. Overloads that
        // differ in their parameters (`Iterator.from(Int)` vs
        // `from(IterableOnce)`) have different keys and all survive.
        // At most one overload per name may take a function parameter. The
        // typer infers a lambda's parameter types from a single expected type,
        // so a second such overload turns `xs.segmentLength(_ < 3)` into an
        // unsolvable overload set. Linearization order means the one kept is
        // the most derived. Overloads that take no function -- `from(Int)` and
        // `from(IterableOnce)` -- are unaffected.
        let has_function = paramss_ty
            .iter()
            .flatten()
            .any(|t| matches!(t, Type::Function { .. }));
        if has_function && *took_function {
            trace(format_args!(
                "{internal}#{name}: skipping a second overload that takes a function"
            ));
            return None;
        }
        let key = format!("{want:?}");
        if !seen_shapes.insert(key) {
            trace(format_args!(
                "{internal}#{name}: skipping an overload shadowed by a more \
                 derived declaration with the same parameters"
            ));
            return None;
        }
        let Some(found) = self.erased_desc(bin, internal, jvm_member, &want) else {
            trace(format_args!(
                "{internal}#{name}/{}: no unambiguous erased descriptor (want {want:?})",
                shape.arity()
            ));
            return None;
        };
        // The member is installed on the class it was asked for, because that
        // is where the typer looks it up. The *call* is a different question:
        // a declaration off the bytecode path is not reachable from the
        // receiver's class, so naming the receiver is a `NoSuchMethodError` at
        // the first invocation. nsc emits `checkcast scala/reflect/api/Constants`
        // and then `invokeinterface scala/reflect/api/Constants.Constant()`;
        // recording the class here is what lets codegen do the same.
        if found.off_the_bytecode_path && found.declared_in != internal {
            trace(format_args!(
                "{internal}#{name}: declared by {}, which the receiver's class file \
                 does not reach -- the call will name it",
                found.declared_in
            ));
            st.get_mut(m).declaring_class = found.declared_in;
            st.get_mut(m).declaring_is_interface = found.declared_by_interface;
        }

        st.get_mut(m).jvm_name = found.desc;
        st.get_mut(m).tparams = tparams;
        st.get_mut(m).params = paramss_sym.iter().flatten().copied().collect();
        st.get_mut(m).paramss = paramss_sym;
        st.get_mut(m).ty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret),
        };
        // A parameter the caller may omit is filled from the class's
        // `<method>$default$<n>` getter. Without it the typer fills nothing and
        // the call goes out with fewer arguments than the descriptor declares,
        // which the verifier rejects -- so a getter we cannot supply makes the
        // whole member ineligible. Done before attaching `m`, so declining
        // leaves the class untouched.
        for slot in default_slots {
            let getter = format!("{name}$default${slot}");
            let ids = self.complete_named(st, bin, class_sym, &getter, true);
            let Some(&gid) = ids.first() else {
                trace(format_args!(
                    "{internal}#{name}: no {getter}, so the default cannot be filled"
                ));
                return None;
            };
            // `default_getter_apply` passes the arguments that precede the
            // defaulted one, truncated to the getter's own arity: scalac emits
            // a *nullary* getter whenever the default does not read an earlier
            // parameter (`SeqOps.lastIndexOf$default$2()` takes nothing though
            // `elem` comes first). Anything between those two shapes is a
            // convention we do not know how to call, and a call we cannot make
            // correctly is worse than a member we do not supply.
            let want_args = slot - 1;
            let got_args = st.get(gid).params.len();
            if got_args != want_args && got_args != 0 {
                trace(format_args!(
                    "{internal}#{name}: {getter} takes {got_args} argument(s), which is \
                     neither none nor the {want_args} that precede the default"
                ));
                return None;
            }
        }

        st.get_mut(m).owner = class_sym;
        st.get_mut(class_sym).members.push(m);
        *took_function = *took_function || has_function;
        Some(m)
    }

    /// Give a class the parents its own pickle declares, if it does not have
    /// them already.
    ///
    /// The prelude declares `immutable.Set` without `collection.Set` above it,
    /// so `Set#&`, whose parameter is `collection.Set[A]`, could be supplied
    /// but never called. Attaching the pickled parents closes that gap, and it
    /// is additive: an existing parent is never removed or replaced, and this
    /// only runs on classes a lookup already failed on.
    fn attach_parents(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        full: &str,
        is_module: bool,
    ) {
        if !self.parented.insert(class_sym.0) {
            return;
        }
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full, is_module)
        }) else {
            return;
        };
        let mut scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }
        for p in sig.parents.clone() {
            let SigType::Ref { sym, .. } = &p else {
                continue;
            };
            if !sym.starts_with("scala.") {
                continue;
            }
            let Some(t) = self.conv(st, bin, &scope, &p) else {
                continue;
            };
            let Type::Class { sym: psym, .. } = &t else {
                continue;
            };
            if *psym == class_sym {
                continue;
            }
            let already = st
                .get(class_sym)
                .parents
                .iter()
                .any(|q| matches!(q, Type::Class { sym: q, .. } if q == psym));
            if already {
                continue;
            }
            // A parent that already has this class above it would make the
            // hierarchy cyclic; the prelude's own shape wins.
            if inherits_from(st, *psym, class_sym) {
                continue;
            }
            trace(format_args!(
                "{full}: attaching pickled parent {}",
                st.get(*psym).name
            ));
            st.get_mut(class_sym).parents.push(t);
        }
    }

    /// The symbol for a library class named in a pickle, creating a stub from
    /// the class's own signature if the symbol table does not have it.
    ///
    /// Without this, any member whose signature mentions a class the prelude
    /// never declared (`scala.collection.IndexedSeq`, `scala.math.Numeric`)
    /// is declined, which is most of the collection API. The stub carries the
    /// real JVM name and the right number of type parameters, which is what
    /// the typer needs to name the type and the backend needs to emit it.
    ///
    /// It deliberately does **not** carry the class's parents: giving a stub a
    /// parent chain would change subtyping for everything, and the prelude's
    /// own hierarchy is the one existing programs are checked against. The cost
    /// is that a stubbed type is only usable as itself (see README).
    fn ensure_class(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        full_name: &str,
        module: bool,
    ) -> Option<SymbolId> {
        let internal = full_name.replace('.', "/");
        // A pickle writes `Outer.Inner` and `pkg.Class` the same way, so the
        // JVM name of a nested class (`Outer$Inner`) cannot be read off the
        // dotted name. Take the first candidate that is really a classfile;
        // `key` decides both the symbol's `jvm_name` and its owner, and
        // getting it wrong invents a package called `Names`.
        let key = {
            let plain = if module {
                format!("{internal}$")
            } else {
                internal.clone()
            };
            scala_rs_pickle::sym::pickle_files_for(full_name, module)
                .into_iter()
                // `pickle_files_for` also offers the *enclosing top-level*
                // class file, because that is where a nested class's pickle
                // actually lives. That candidate is right for reading bytes and
                // wrong for naming a class: accepting it made
                // `scala.reflect.api.Constants.Constant` -- an abstract type
                // member, with no class file of its own -- resolve to the
                // enclosing trait `Constants`, so `Literal(Constant(42))` was
                // checked against a parameter of the wrong type and no erased
                // descriptor matched. A candidate only names this class if it
                // ends with this class's own simple name.
                .filter(|c| names_class(c, full_name))
                .find(|c| bin.find_class(c).ok().flatten().is_some())
                .unwrap_or(plain)
        };
        if let Some(id) = self.stubs.get(&key) {
            return Some(*id);
        }
        // A symbol already in the table wins, whatever shape it is in. An
        // earlier version gave an under-specified one (`scala/collection/Seq`,
        // entered by `find_or_stub_java_class` with no type parameters) the
        // parameters its pickle declares, so that `Seq[B]` would match it. That
        // unlocked `diff` / `intersect` / `union` / `indexOfSlice`, but it also
        // mutates a symbol the prelude built and the rest of the typer already
        // reasons about: with `Seq` reshaped, `xs.segmentLength(_ < 3)` and
        // `xs.scanRight(0)(_ + _)` -- both hand-written prelude members --
        // stopped resolving. Breaking a member that works is worse than not
        // supplying one that does not, so the table is left alone.
        if let Some(id) = crate::classpath::find_by_jvm(st, &key) {
            self.stubs.insert(key, id);
            self.give_stub_its_kinds(st, bin, id, full_name, module);
            return Some(id);
        }
        // Outside the library, hand the placeholder to the ordinary classfile
        // loader rather than building one here. `find_or_stub_java_class`
        // marks it `JAVA`, which is what makes `ensure_java_loaded` complete it
        // from its own classfile the moment the program names it; a stub built
        // below would keep its empty member list forever. It still gets its
        // kinds now, since that is what the signature being converted is about
        // to apply.
        if !full_name.starts_with("scala.") {
            if !self.has_pickle(bin, full_name, module) {
                return None;
            }
            let id = crate::classpath::find_or_stub_java_class(st, &key);
            // That function also answers by simple name, and a companion's
            // placeholder -- `Async` standing for `.../Async$` -- is not the
            // trait this signature names. Handing it back made
            // `type Async[F[_]] = cats.effect.kernel.Async[F]` resolve to a
            // class with no members at all, so `F.unit` stopped being one.
            if st.get(id).jvm_name != key {
                return None;
            }
            self.stubs.insert(key, id);
            self.give_stub_its_kinds(st, bin, id, full_name, module);
            return Some(id);
        }
        let sig = {
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full_name, module).ok()?
        };
        let base = key.strip_suffix('$').unwrap_or(&key).to_string();
        let (pkg_jvm, simple) = match base.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), base.clone()),
        };
        if simple.is_empty() {
            return None;
        }
        let owner = crate::classpath::ensure_package(st, &pkg_jvm);
        let id = if module {
            let cls = st.alloc(
                format!("{simple}$"),
                owner,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                &key,
            );
            let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, &key);
            st.get_mut(m).ty = Type::ModuleRef(cls);
            st.get_mut(cls).ty = Type::ModuleRef(cls);
            cls
        } else {
            let mut flags = Flags::EMPTY;
            if sig.flags & pflags::TRAIT != 0 || sig.flags & pflags::INTERFACE != 0 {
                flags = flags.with(Flags::INTERFACE).with(Flags::TRAIT);
            }
            if sig.flags & pflags::ABSTRACT != 0 {
                flags = flags.with(Flags::ABSTRACT);
            }
            let id = st.alloc(&simple, owner, SymKind::Class, flags, &key);
            let tparams: Vec<SymbolId> = sig
                .tparams
                .iter()
                .map(|tp| {
                    let t = st.alloc(&tp.name, id, SymKind::TypeParam, Flags::EMPTY, "");
                    st.get_mut(t).ty = Type::TypeParam(t);
                    set_tparam_arity(st, t, tparam_arity(tp));
                    t
                })
                .collect();
            st.get_mut(id).tparams = tparams;
            st.get_mut(id).parents = vec![Type::AnyRef];
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            id
        };
        trace(format_args!("stubbed class {full_name} (module={module})"));
        self.stubs.insert(key, id);
        Some(id)
    }

    /// Give a *placeholder* symbol the type parameters its pickle declares.
    ///
    /// `find_or_stub_java_class` enters a bare symbol for every name a parent
    /// list or a descriptor mentions, without reading the classfile. Outside
    /// the library those placeholders are what a pickled signature keeps
    /// running into: `cats.effect.kernel.Sync` is entered from `Ref`'s parent
    /// list with no type parameters at all, so `Sync[F]` is "applied to 1
    /// argument but the symbol has 0" and every `Ref.of` / `Ref.ofEffect` /
    /// `Ref.lens` is declined for it.
    ///
    /// Only a symbol nothing has filled in yet, and never one of the
    /// library's: a class the typer has really loaded already has its
    /// parameters, and reshaping a prelude class is what `ensure_class`
    /// deliberately refuses to do.
    fn give_stub_its_kinds(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        id: SymbolId,
        full_name: &str,
        module: bool,
    ) {
        let jvm = st.get(id).jvm_name.clone();
        if jvm.starts_with("scala/") || jvm.starts_with("java/") || jvm.starts_with("javax/") {
            return;
        }
        if !st.get(id).tparams.is_empty() || !st.get(id).members.is_empty() {
            return;
        }
        let Ok(sig) = ({
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, full_name, module)
        }) else {
            return;
        };
        if sig.tparams.is_empty() {
            return;
        }
        trace(format_args!(
            "{full_name}: giving the placeholder symbol its {} pickled type parameter(s)",
            sig.tparams.len()
        ));
        adopt_tparam_kinds(st, id, &sig);
    }

    /// The declared descriptor of `name`, searched from `internal` up through
    /// superclasses and interfaces. `want` is one slot per value parameter,
    /// holding the erased descriptor we expect where we can name it.
    ///
    /// Returns `None` when nothing matches, or when candidates still tie after
    /// the parameter descriptors are compared: picking one arbitrarily would
    /// silently call the wrong method.
    fn erased_desc(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        name: &str,
        want: &[Option<String>],
    ) -> Option<ErasedDecl> {
        let arity = want.len();
        let mut seen: HashSet<String> = HashSet::new();
        // Each frontier entry remembers whether it was reached through a hop
        // the JVM cannot make -- see `ErasedDecl::off_the_bytecode_path`.
        let mut level = vec![(internal.to_string(), false)];
        for _ in 0..32 {
            if level.is_empty() {
                return None;
            }
            let mut hits: Vec<ErasedDecl> = Vec::new();
            let mut next = Vec::new();
            for (cn, off_path) in &level {
                let off_path = *off_path;
                if !seen.insert(cn.clone()) {
                    continue;
                }
                let Some(jc) = self.java_class(bin, cn) else {
                    continue;
                };
                let declared_by_interface = crate::javaclass::is_java_interface(jc.access);
                let mut parents: Vec<String> = Vec::new();
                for jm in &jc.methods {
                    if jm.name != name || jm.access & (ACC_BRIDGE | ACC_SYNTHETIC | ACC_STATIC) != 0
                    {
                        continue;
                    }
                    if desc_arity(&jm.desc) == Some(arity)
                        && params_match(&jm.desc, want)
                        && !hits.iter().any(|h| h.desc == jm.desc)
                    {
                        hits.push(ErasedDecl {
                            desc: jm.desc.clone(),
                            declared_in: cn.clone(),
                            declared_by_interface,
                            off_the_bytecode_path: off_path,
                        });
                    }
                }
                if let Some(s) = &jc.super_name {
                    if s != "java/lang/Object" {
                        parents.push(s.clone());
                    }
                }
                parents.extend(jc.interfaces.iter().cloned());
                let bare = parents.is_empty();
                if jc.super_name.is_some() && bare {
                    parents.push("java/lang/Object".to_string());
                }
                // A Scala trait whose members are all abstract can compile to
                // an interface that lists no parents at all --
                // `api/JavaUniverse.class` declares four methods and
                // `interfaces: 0`, though the trait extends `Universe`. Its
                // ancestry then exists only in the pickle. Only that case is
                // topped up: reading the pickled parents of a class whose
                // bytecode *does* declare its hierarchy widens the search to
                // ancestors the JVM does not see, and the extra `map`
                // descriptors that turns up make the answer ambiguous
                // (`Map#map` regresses).
                next.extend(parents.into_iter().map(|p| (p, off_path)));
                if bare {
                    next.extend(
                        self.pickled_parent_files(bin, cn)
                            .into_iter()
                            .map(|p| (p, true)),
                    );
                }
            }
            match hits.len() {
                0 => {}
                1 => return Some(hits.remove(0)),
                _ => return None,
            }
            level = next;
        }
        None
    }

    /// The classfiles of the parents `internal`'s *pickle* declares.
    ///
    /// Empty for anything outside the standard library and for any class whose
    /// pickle is missing: this only tops up the bytecode hierarchy, it never
    /// replaces it.
    fn pickled_parent_files(&mut self, bin: &mut BinaryIndex, internal: &str) -> Vec<String> {
        if !internal.starts_with("scala/") {
            return Vec::new();
        }
        let module = internal.ends_with('$');
        let full = internal.trim_end_matches('$').replace('/', ".");
        let sig = {
            let mut src = BinSource(bin);
            match self.sigs.class_sig(&mut src, &full, module) {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            }
        };
        let mut out = Vec::new();
        for p in &sig.parents {
            let SigType::Ref { sym, .. } = p else {
                continue;
            };
            if let Some(f) = self.class_file_of(bin, sym) {
                out.push(f);
            }
        }
        out
    }

    /// Whether a pickle describes `full_name`.
    fn has_pickle(&mut self, bin: &mut BinaryIndex, full_name: &str, module: bool) -> bool {
        let mut src = BinSource(bin);
        let r = self.sigs.class_sig(&mut src, full_name, module);
        if let Err(e) = &r {
            trace(format_args!(
                "has_pickle {full_name} module={module}: {e:?}"
            ));
        }
        r.is_ok()
    }

    /// The classfile that really holds `full_name`, or `None` if there is none.
    fn class_file_of(&mut self, bin: &mut BinaryIndex, full_name: &str) -> Option<String> {
        scala_rs_pickle::sym::pickle_files_for(full_name, false)
            .into_iter()
            .find(|c| bin.find_class(c).ok().flatten().is_some())
    }

    fn java_class(&mut self, bin: &mut BinaryIndex, internal: &str) -> Option<&JavaClass> {
        if !self.classes.contains_key(internal) {
            let parsed = bin
                .find_class(internal)
                .ok()
                .flatten()
                .and_then(|b| parse_java_classfile(&b).ok());
            self.classes.insert(internal.to_string(), parsed);
        }
        self.classes.get(internal).and_then(|c| c.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Pickled signature -> method shape
// ---------------------------------------------------------------------------

struct ShapeTParam {
    name: String,
    lo: Option<SigType>,
    hi: Option<SigType>,
    /// Its own kind arity: 1 for the `G[_]` of `def mapK[G[_]](…)`.
    arity: usize,
}

struct Param {
    name: String,
    ty: SigType,
    by_name: bool,
    /// Raw pickled flags, for `DEFAULTPARAM`.
    flags: u64,
}

struct Clause {
    params: Vec<Param>,
    implicit: bool,
}

struct Shape {
    tparams: Vec<ShapeTParam>,
    clauses: Vec<Clause>,
    ret: SigType,
}

impl Shape {
    fn arity(&self) -> usize {
        self.clauses.iter().map(|c| c.params.len()).sum()
    }
}

/// Peel `POLYtpe` / `METHODtpe` layers into type parameters and parameter
/// clauses. nsc writes a parameterless `def` as a `POLYtpe` with no type
/// parameters (`NullaryMethodType`), which becomes an empty clause list.
fn read_shape(t: &SigType) -> Option<Shape> {
    let mut tparams = Vec::new();
    let mut clauses = Vec::new();
    let mut cur = t;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 16 {
            return None;
        }
        match cur {
            SigType::Poly {
                tparams: tps,
                result,
            } => {
                for tp in tps {
                    let (lo, hi) = match &tp.bounds {
                        SigType::Bounds { lo, hi } => (Some((**lo).clone()), Some((**hi).clone())),
                        _ => (None, None),
                    };
                    tparams.push(ShapeTParam {
                        name: tp.name.clone(),
                        lo,
                        hi,
                        arity: tparam_arity(tp),
                    });
                }
                cur = result;
            }
            SigType::Method {
                params,
                implicit,
                result,
            } => {
                clauses.push(Clause {
                    params: params
                        .iter()
                        .map(|p| Param {
                            name: p.name.clone(),
                            ty: p.ty.clone(),
                            by_name: p.by_name,
                            flags: p.flags,
                        })
                        .collect(),
                    implicit: *implicit,
                });
                cur = result;
            }
            other => {
                return Some(Shape {
                    tparams,
                    clauses,
                    ret: other.clone(),
                })
            }
        }
    }
}

/// `def max[B >: A](implicit ord: Ordering[B]): A` has nothing at the call site
/// to infer `B` from; scalac resolves it to the lower bound, `A`. Do the same
/// here, and drop the parameter.
///
/// Without this the typer cannot solve `Ordering[B]`, and instead of failing it
/// eta-expands `xs.max` into a function value — a silently wrong program. Any
/// type parameter left undetermined after this pass makes the whole member
/// ineligible, so that shape can never reach the user.
fn pin_undetermined_tparams(shape: Shape) -> Option<Shape> {
    let determined: HashSet<String> = shape
        .clauses
        .iter()
        .filter(|c| !c.implicit)
        .flat_map(|c| c.params.iter())
        .flat_map(|p| mentioned(&p.ty))
        .collect();
    let mut pin: HashMap<String, SigType> = HashMap::new();
    let mut kept = Vec::new();
    for tp in &shape.tparams {
        if determined.contains(&tp.name) {
            kept.push(ShapeTParam {
                name: tp.name.clone(),
                lo: tp.lo.clone(),
                hi: tp.hi.clone(),
                arity: tp.arity,
            });
            continue;
        }
        match &tp.lo {
            Some(lo) if !matches!(lo, SigType::Ref { sym, .. } if sym == "scala.Nothing") => {
                pin.insert(tp.name.clone(), lo.clone());
            }
            // No lower bound to pin it to, but the *result* names it: the call
            // site can still determine it, from an explicit type application
            // (`classTag[Short]`) or from the expected type. Keep the member.
            // `max[B >: A](implicit ord: Ordering[B]): A` is the other shape --
            // `B` is nowhere in the result, and it does have a lower bound.
            _ if mentioned(&shape.ret).contains(&tp.name) => {
                kept.push(ShapeTParam {
                    name: tp.name.clone(),
                    lo: tp.lo.clone(),
                    hi: tp.hi.clone(),
                    arity: tp.arity,
                });
            }
            // Unconstrained and undeterminable: refuse the member rather than
            // hand the typer something it will silently eta-expand.
            _ => return None,
        }
    }
    if pin.is_empty() {
        return Some(shape);
    }
    Some(Shape {
        tparams: kept,
        clauses: shape
            .clauses
            .iter()
            .map(|c| Clause {
                implicit: c.implicit,
                params: c
                    .params
                    .iter()
                    .map(|p| Param {
                        name: p.name.clone(),
                        ty: scala_rs_pickle::sym::apply_subst(&p.ty, &pin),
                        by_name: p.by_name,
                        flags: p.flags,
                    })
                    .collect(),
            })
            .collect(),
        ret: scala_rs_pickle::sym::apply_subst(&shape.ret, &pin),
    })
}

/// Give a binary class's type parameters the kinds its pickle declares.
///
/// A JVM generic signature writes `trait Monad[F[_]]` as `<F:Ljava/lang/Object;>`
/// — `F` is a proper type there, and there is no way to say otherwise. So the
/// symbols are already allocated, in the right order and under the right names;
/// what they are missing is the parameters *they* take. If the two disagree
/// about how many there are, the JVM signature is left alone: a mismatch means
/// this is not the class the pickle describes.
fn adopt_tparam_kinds(
    st: &mut SymbolTable,
    class_sym: SymbolId,
    sig: &scala_rs_pickle::sym::ClassSig,
) {
    let ids = st.get(class_sym).tparams.clone();
    if ids.is_empty() {
        // No generic signature at all (or none we parsed): the pickle is the
        // only description there is.
        if sig.tparams.is_empty() {
            return;
        }
        let mut fresh = Vec::new();
        for tp in &sig.tparams {
            let id = st.alloc(&tp.name, class_sym, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(id).ty = Type::TypeParam(id);
            set_tparam_arity(st, id, tparam_arity(tp));
            fresh.push(id);
        }
        st.get_mut(class_sym).tparams = fresh;
        return;
    }
    if ids.len() != sig.tparams.len() {
        return;
    }
    for (id, tp) in ids.into_iter().zip(&sig.tparams) {
        if st.get(id).tparams.is_empty() {
            set_tparam_arity(st, id, tparam_arity(tp));
        }
    }
}

/// Make `id` a type constructor of `arity` parameters. The names are
/// placeholders: only the count is ever read (`SymbolTable::kind_arity`).
fn set_tparam_arity(st: &mut SymbolTable, id: SymbolId, arity: usize) {
    if arity == 0 {
        return;
    }
    let inner: Vec<SymbolId> = (0..arity)
        .map(|i| {
            let x = st.alloc(format!("_${i}"), id, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(x).ty = Type::TypeParam(x);
            x
        })
        .collect();
    st.get_mut(id).tparams = inner;
}

/// A type parameter's own kind arity. nsc pickles `F[_]` as a `POLYtpe` over
/// the bounds, so the number of quantified names is the arity; a proper type
/// parameter has plain `Bounds` and arity 0.
fn tparam_arity(tp: &scala_rs_pickle::sym::TParam) -> usize {
    match &tp.bounds {
        SigType::Poly { tparams, .. } => tparams.len(),
        _ => 0,
    }
}

/// Every bare name a type mentions, so we can tell which type parameters an
/// explicit argument would determine.
fn mentioned(t: &SigType) -> Vec<String> {
    let mut out = Vec::new();
    walk(t, &mut out, 0);
    out
}

fn walk(t: &SigType, out: &mut Vec<String>, depth: u32) {
    if depth > 24 {
        return;
    }
    let d = depth + 1;
    match t {
        SigType::Ref { sym, args } => {
            out.push(sym.clone());
            for a in args {
                walk(a, out, d);
            }
        }
        SigType::Annotated(x) => walk(x, out, d),
        SigType::Bounds { lo, hi } => {
            walk(lo, out, d);
            walk(hi, out, d);
        }
        SigType::Method { params, result, .. } => {
            for p in params {
                walk(&p.ty, out, d);
            }
            walk(result, out, d);
        }
        SigType::Poly { result, .. } | SigType::Existential { result, .. } => walk(result, out, d),
        SigType::Refined { parents, decls } => {
            for p in parents {
                walk(p, out, d);
            }
            for m in decls {
                walk(&m.ty, out, d);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// SigType -> scala_rs_parser::Type
// ---------------------------------------------------------------------------

/// Map a pickled type onto the typer's. `None` means "cannot express this",
/// and the caller then declines to supply the member.
impl PickleSupply {
    fn conv(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
    ) -> Option<Type> {
        self.conv_at(st, bin, scope, t, 0)
    }

    fn conv_at(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        t: &SigType,
        depth: u32,
    ) -> Option<Type> {
        if depth > 24 {
            return None;
        }
        let d = depth + 1;
        match t {
            SigType::Annotated(inner) => self.conv_at(st, bin, scope, inner, d),
            SigType::Existential { quantified, result } => {
                // `List[_]`: the quantified variables stand for wildcards.
                let mut inner = scope.clone();
                for q in quantified {
                    inner.insert(q.name.clone(), Type::Wildcard);
                }
                self.conv_at(st, bin, &inner, result, d)
            }
            SigType::Ref { sym, args } => self.conv_ref(st, bin, scope, sym, args, d),
            // `this.type` widens to the receiver applied to its own type
            // parameters; `type_select` then substitutes the receiver's
            // arguments into it, so `b ++= xs` on a `Builder[Int, List[Int]]`
            // yields that `Builder` and not `Growable`.
            SigType::This(_) => self.self_ty.clone(),
            // A `val`'s own type is fine, but the remaining forms (singletons,
            // `super`, bare bounds, refinements, literal types) have no
            // faithful counterpart here yet.
            _ => None,
        }
    }

    fn conv_ref(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        sym: &str,
        args: &[SigType],
        d: u32,
    ) -> Option<Type> {
        if let Some(bound) = scope.get(sym) {
            if args.is_empty() {
                return Some(bound.clone());
            }
            // A type parameter applied to arguments (`F[A]` for an `F[_]`).
            // `Type::Applied` is exactly that, but only when the constructor
            // really takes that many parameters: a wildcard from an
            // existential, or a parameter whose kind we never recovered, would
            // make `F[A]` a type nothing can be checked against.
            let bound = bound.clone();
            if st.kind_arity(&bound) != args.len() {
                trace(format_args!(
                    "{sym}: applied to {} argument(s) but is not a constructor of that kind",
                    args.len()
                ));
                return None;
            }
            let a = self.conv_all(st, bin, scope, args, d)?;
            return Some(Type::Applied {
                ctor: Box::new(bound),
                args: a,
            });
        }
        match sym {
            "scala.Unit" => return Some(Type::Unit),
            "scala.Boolean" => return Some(Type::Boolean),
            "scala.Byte" => return Some(Type::Byte),
            "scala.Short" => return Some(Type::Short),
            "scala.Int" => return Some(Type::Int),
            "scala.Long" => return Some(Type::Long),
            "scala.Float" => return Some(Type::Float),
            "scala.Double" => return Some(Type::Double),
            "scala.Char" => return Some(Type::Char),
            "scala.Any" => return Some(Type::Any),
            "scala.AnyRef" | "java.lang.Object" => return Some(Type::AnyRef),
            "scala.AnyVal" => return Some(Type::AnyVal),
            "scala.Nothing" => return Some(Type::Nothing),
            "scala.Null" => return Some(Type::Null),
            "java.lang.String" | "scala.Predef.String" => return Some(Type::String),
            "scala.Array" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::Array(Box::new(e)));
            }
            "scala.<byname>" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::ByName(Box::new(e)));
            }
            "scala.<repeated>" => {
                let a = self.conv_all(st, bin, scope, args, d)?;
                return a.into_iter().next().map(|e| Type::Repeated(Box::new(e)));
            }
            _ => {}
        }
        if let Some(n) = sym.strip_prefix("scala.Function") {
            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                let mut a = self.conv_all(st, bin, scope, args, d)?;
                let ret = a.pop()?;
                return Some(Type::Function {
                    params: a,
                    ret: Box::new(ret),
                });
            }
        }
        if let Some(n) = sym.strip_prefix("scala.Tuple") {
            if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
                return Some(Type::Tuple(self.conv_all(st, bin, scope, args, d)?));
            }
        }
        // A type member named from inside its own class is pickled bare:
        // `Internals.internal` returns `Internal`, not `...Internals.Internal`.
        // There is no class by that name, so without this the member is
        // declined and `u.internal` -- the door to the whole reification API
        // -- is not a member of anything.
        if args.is_empty() && !sym.contains('.') && scope.get(sym).is_none() {
            if let Some(t) = self.self_type_member(st, bin, sym, d) {
                return Some(t);
            }
        }
        // `scala.package.List` and friends: package-object type aliases, which
        // pickles refer to by the alias, not the target. Expand through the
        // owner's own pickle rather than hard-coding a table. Tried before
        // `ensure_class` so an alias is not mistaken for a class of its own.
        let internal = sym.replace('.', "/");
        if crate::classpath::find_by_jvm(st, &internal).is_none() {
            if let Some(expanded) = self.expand_alias(st, bin, scope, sym, args, d) {
                return Some(expanded);
            }
        }
        let Some(cls) = self.ensure_class(st, bin, sym, false) else {
            // `scala.reflect.api.Trees.Tree` names an *abstract type member* of
            // trait `Trees`, not a class: there is no such classfile and never
            // will be. Almost the whole reflection API is written this way
            // (`Tree`, `Name`, `Symbol`, `Type`, `Position`, `FlagSet`), so
            // declining here would make the API unusable.
            if args.is_empty() {
                if let Some(t) = self.abstract_type_member(st, bin, sym, d) {
                    return Some(t);
                }
            }
            // Remembered rather than merely declined: the typer can often load
            // the classfile itself (any package on `-cp`, not just `scala.*`)
            // and ask again.
            if !self.unresolved_refs.iter().any(|s| s == sym) {
                self.unresolved_refs.push(sym.to_string());
            }
            return None;
        };
        let a = self.conv_all(st, bin, scope, args, d)?;
        if a.len() != st.get(cls).tparams.len() {
            trace(format_args!(
                "{sym}: applied to {} arguments but the symbol has {}",
                a.len(),
                st.get(cls).tparams.len()
            ));
            return None;
        }
        Some(Type::Class { sym: cls, args: a })
    }

    fn conv_all(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        args: &[SigType],
        d: u32,
    ) -> Option<Vec<Type>> {
        args.iter()
            .map(|a| self.conv_at(st, bin, scope, a, d))
            .collect()
    }

    /// `T` as an abstract type member of the class whose member is being
    /// completed, or of anything it inherits from.
    ///
    /// `None` outside a completion, and for any name no ancestor declares --
    /// a bare `Ref` is far more often a type parameter or a class, and this
    /// runs only after both of those have been ruled out.
    fn self_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        name: &str,
        d: u32,
    ) -> Option<Type> {
        let cls = match &self.self_ty {
            Some(Type::Class { sym, .. }) => *sym,
            _ => return None,
        };
        let internal = st.get(cls).jvm_name.clone();
        if !internal.starts_with("scala/") {
            return None;
        }
        let module = internal.ends_with('$');
        // The receiver, then each class it is nested in: `ConstantExtractor`
        // is declared inside trait `Constants`, and its `apply` returns that
        // trait's `Constant`, not one of its own.
        let mut owners: Vec<String> = Vec::new();
        let mut cur = internal.trim_end_matches('$').replace('/', ".");
        loop {
            owners.push(cur.replace('$', "."));
            match cur.rsplit_once('$') {
                Some((outer, _)) => cur = outer.to_string(),
                None => break,
            }
        }
        for owner in owners {
            let lin = {
                let mut src = BinSource(bin);
                let mut errs = Vec::new();
                self.sigs.linearization(&mut src, &owner, module, &mut errs)
            };
            for step in lin {
                let qualified = format!("{}.{name}", step.class_name);
                if let Some(t) = self.abstract_type_member(st, bin, &qualified, d) {
                    return Some(t);
                }
            }
        }
        None
    }

    /// Resolve `Owner.T` where `T` is an **abstract type member** of `Owner`
    /// (`type Tree >: Null <: TreeApi`), installing it as a `TypeMember`
    /// symbol on `Owner` so the typer can see it through a path.
    ///
    /// Only abstract members are handled here; `type T = U` already goes
    /// through `expand_alias`, which substitutes the right-hand side.
    ///
    /// The upper bound is converted too, and a member whose bound cannot be
    /// converted is installed *unbounded* rather than declined: an opaque
    /// `u.Tree` that can be named and passed around is right, and losing the
    /// bound only means fewer members are reachable through it, never a wrong
    /// one.
    fn abstract_type_member(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        sym: &str,
        d: u32,
    ) -> Option<Type> {
        let (owner_name, member) = sym.rsplit_once('.')?;
        if member.is_empty() || !member.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        let owner = self.ensure_class(st, bin, owner_name, false)?;
        if let Some(id) = st
            .lookup_member(owner, member)
            .into_iter()
            .find(|&s| st.get(s).kind == SymKind::TypeMember)
        {
            return Some(Type::TypeMember(id));
        }
        let sig = {
            let mut src = BinSource(bin);
            self.sigs.class_sig(&mut src, owner_name, false).ok()?
        };
        let m = sig
            .members
            .iter()
            .find(|m| m.name == member && m.kind == MemberKind::AbstractType)?
            .clone();
        let id = st.alloc(member, owner, SymKind::TypeMember, Flags::EMPTY, "");
        st.get_mut(owner).members.push(id);
        // Entered before the bound is converted: `type Tree >: Null <: TreeApi`
        // and `type TreeApi` can refer to each other, and without the symbol
        // being visible first that recurses until the depth limit.
        if let SigType::Bounds { lo, hi } = &m.ty {
            let scope = HashMap::new();
            let (lo, hi) = (lo.clone(), hi.clone());
            // A bound is written in *its owner's* vocabulary, not the
            // receiver's. `type Ident >: Null <: IdentApi with RefTree` names
            // `RefTree` with a bare name, because it is another abstract type
            // member of the same trait `Trees`; resolving it against whatever
            // member happened to be under completion finds nothing when that
            // member lives elsewhere (`Internals.SyntacticTermIdentExtractor`).
            // The bound then loses `RefTree`, and with it the only path from
            // `Ident` to `Tree` -- so nothing built out of an `Ident`
            // typechecks. Point `self_ty` at the owner for the conversion.
            let outer = self.self_ty.replace(Type::Class {
                sym: owner,
                args: Vec::new(),
            });
            if let Some(h) = self.conv_upper_bound(st, bin, &scope, &hi, d) {
                if !matches!(h, Type::Any | Type::AnyRef) {
                    st.get_mut(id).bound_hi = Some(h);
                }
            }
            if let Some(l) = self.conv_at(st, bin, &scope, &lo, d) {
                if !matches!(l, Type::Nothing) {
                    st.get_mut(id).bound_lo = Some(l);
                }
            }
            self.self_ty = outer;
        }
        trace(format_args!("abstract type member {sym}"));
        Some(Type::TypeMember(id))
    }

    /// The upper bound of an abstract type member.
    ///
    /// Same as `conv_at`, except that a **compound** bound is kept rather than
    /// declined. The reflect API is written entirely in these:
    /// `type Select >: Null <: SelectApi with RefTree`, and `RefTree` in turn
    /// leads to `Tree`. Dropping the bound because `conv_at` has no answer for
    /// a refinement leaves `Select` unrelated to `Tree`, and then nothing built
    /// out of `Tree`s -- every `Syntactic*` call reification would emit --
    /// typechecks. Declarations inside the refinement are dropped: only the
    /// parents decide conformance, and inventing members here would be a
    /// guess. A parent that does not convert is skipped rather than failing the
    /// bound, on the same principle as the caller's: fewer members reachable
    /// through the type is right, a wrong one is not.
    fn conv_upper_bound(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        hi: &SigType,
        d: u32,
    ) -> Option<Type> {
        let SigType::Refined { parents, .. } = hi else {
            return self.conv_at(st, bin, scope, hi, d);
        };
        let mut ps: Vec<Type> = Vec::new();
        for p in parents {
            let Some(t) = self.conv_at(st, bin, scope, p, d) else {
                continue;
            };
            if matches!(t, Type::Any | Type::AnyRef) || ps.contains(&t) {
                continue;
            }
            ps.push(t);
        }
        match ps.len() {
            0 => None,
            1 => Some(ps.remove(0)),
            _ => Some(Type::Refined {
                parents: ps,
                decls: Vec::new(),
            }),
        }
    }

    /// Resolve `owner.Name[args]` where `Name` is a type alias declared on
    /// `owner` (typically a package object), by reading `owner`'s pickle and
    /// substituting the alias's own type parameters.
    fn expand_alias(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        scope: &HashMap<String, Type>,
        sym: &str,
        args: &[SigType],
        d: u32,
    ) -> Option<Type> {
        let (owner, simple) = sym.rsplit_once('.')?;
        let sig = {
            let mut src = BinSource(bin);
            self.sigs
                .class_sig(&mut src, owner, true)
                .or_else(|_| self.sigs.class_sig(&mut src, owner, false))
                .ok()?
        };
        let alias = sig
            .members
            .iter()
            .find(|m| m.name == simple && m.kind == MemberKind::TypeAlias)?
            .clone();
        // A parameterised alias (`type List[+A] = immutable.List[A]`) binds its
        // own parameters to our arguments; a plain one has none.
        let (tps, target) = match &alias.ty {
            SigType::Poly { tparams, result } => (tparams.clone(), (**result).clone()),
            other => (Vec::new(), other.clone()),
        };
        if tps.len() != args.len() {
            return None;
        }
        let mut map: HashMap<String, SigType> = HashMap::new();
        for (tp, a) in tps.iter().zip(args.iter()) {
            map.insert(tp.name.clone(), a.clone());
        }
        let target = scala_rs_pickle::sym::apply_subst(&target, &map);
        // The substituted arguments are still written in the caller's
        // vocabulary, so the caller's scope is what finishes the job.
        self.conv_at(st, bin, scope, &target, d)
    }
}

/// `f$default$2` names the getter for `f`'s second parameter's default.
fn is_default_getter(name: &str) -> bool {
    let Some((_, n)) = name.rsplit_once("$default$") else {
        return false;
    };
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
}

/// Whether `cls` already has `target` somewhere above it.
pub(crate) fn inherits_from(st: &SymbolTable, cls: SymbolId, target: SymbolId) -> bool {
    let mut seen: Vec<u32> = Vec::new();
    let mut work = vec![cls];
    while let Some(c) = work.pop() {
        if c == target {
            return true;
        }
        if seen.contains(&c.0) || seen.len() > 256 {
            continue;
        }
        seen.push(c.0);
        for p in &st.get(c).parents {
            if let Some(ps) = st.class_sym_of(p) {
                work.push(ps);
            }
        }
    }
    false
}

/// The JVM descriptor a converted parameter type erases to, where that is
/// certain. `None` means "some reference type" -- a type parameter, `Any`, or
/// anything else that erases to `Object` or to a class we cannot pin down --
/// and matches any reference slot.
/// Whether the class file `candidate` (a JVM internal name) is the one that
/// *declares* `full_name`, rather than merely an enclosing class whose file
/// carries its pickle.
///
/// `a/b/Outer$Inner` names `a.b.Outer.Inner`; `a/b/Outer` does not. Trailing
/// `$` (a module class) does not change the simple name.
fn names_class(candidate: &str, full_name: &str) -> bool {
    let Some(simple) = full_name.rsplit('.').next() else {
        return false;
    };
    let last = candidate.rsplit('/').next().unwrap_or(candidate);
    let last = last.strip_suffix('$').unwrap_or(last);
    last == simple || last.rsplit('$').next() == Some(simple)
}

fn erased_param_desc(st: &SymbolTable, ty: &Type) -> Option<String> {
    match ty {
        Type::Boolean => Some("Z".into()),
        Type::Byte => Some("B".into()),
        Type::Short => Some("S".into()),
        Type::Char => Some("C".into()),
        Type::Int => Some("I".into()),
        Type::Long => Some("J".into()),
        Type::Float => Some("F".into()),
        Type::Double => Some("D".into()),
        Type::Unit => Some("V".into()),
        Type::String => Some("Ljava/lang/String;".into()),
        Type::Function { params, .. } => Some(format!("Lscala/Function{};", params.len())),
        Type::ByName(_) => Some("Lscala/Function0;".into()),
        Type::Class { sym, .. } => {
            let n = st.get(*sym).jvm_name.clone();
            if n.is_empty() || n.starts_with('[') {
                None
            } else {
                Some(format!("L{n};"))
            }
        }
        _ => None,
    }
}

/// Whether a candidate descriptor's parameters agree with the slots we could
/// name. An unnamed slot matches any reference parameter but not a primitive:
/// that is what separates `from(int)` from `from(IterableOnce)`.
fn params_match(desc: &str, want: &[Option<String>]) -> bool {
    let Some(got) = desc_params(desc) else {
        return false;
    };
    if got.len() != want.len() {
        return false;
    }
    got.iter().zip(want).all(|(g, w)| match w {
        Some(w) => g == w,
        None => g.starts_with('L') || g.starts_with('['),
    })
}

/// Split a method descriptor's parameter list into individual descriptors.
fn desc_params(desc: &str) -> Option<Vec<String>> {
    let b = desc.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < b.len() && b[i] != b')' {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
        }
        i += 1;
        out.push(desc[start..i].to_string());
    }
    if i >= b.len() {
        return None;
    }
    Some(out)
}

/// Number of parameters in a JVM method descriptor.
fn desc_arity(desc: &str) -> Option<usize> {
    let b = desc.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut i = 1;
    let mut n = 0;
    while i < b.len() && b[i] != b')' {
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
        }
        i += 1;
        n += 1;
    }
    if i >= b.len() {
        return None;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypecheckOptions;

    fn jar() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        p.is_file().then_some(p)
    }

    fn library_opts(jar: &std::path::Path) -> TypecheckOptions {
        TypecheckOptions {
            library_abi: true,
            binary_path: vec![jar.to_path_buf()],
            ..TypecheckOptions::default()
        }
    }

    /// A member the prelude declares by hand must keep coming from the
    /// prelude: completion runs only after resolution failed, so it can add
    /// members but never replace one. A supplied member is recognisable by the
    /// JVM descriptor parked in its `jvm_name`.
    #[test]
    fn the_prelude_wins_over_the_pickle() {
        let Some(jar) = jar() else {
            eprintln!("skip: scala-library jar not present");
            return;
        };
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    xs.map(x => x + 1)
    xs.tails
  }
}
"#;
        let (_t, st, diags) = crate::typecheck_str_opts(src, &library_opts(&jar));
        assert!(
            !crate::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let list = st.list_sym;
        let maps: Vec<_> = st
            .get(list)
            .members
            .iter()
            .filter(|&&m| st.get(m).name == "map")
            .collect();
        assert_eq!(
            maps.len(),
            1,
            "prelude List#map was duplicated by the pickle"
        );
        assert!(
            st.get(*maps[0]).jvm_name.is_empty(),
            "prelude List#map was replaced by a pickle-supplied one"
        );
        // ...while a member the prelude does not have does come from the pickle.
        let tails: Vec<_> = st
            .get(list)
            .members
            .iter()
            .filter(|&&m| st.get(m).name == "tails")
            .collect();
        assert_eq!(tails.len(), 1, "expected exactly one supplied tails");
        assert!(
            st.get(*tails[0]).jvm_name.starts_with('('),
            "List#tails should carry an erased descriptor, got {:?}",
            st.get(*tails[0]).jvm_name
        );
    }

    /// Nothing is read until a lookup actually misses.
    #[test]
    fn nothing_is_supplied_when_nothing_is_missing() {
        let Some(jar) = jar() else {
            eprintln!("skip: scala-library jar not present");
            return;
        };
        let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    xs.foreach(x => println(x))
  }
}
"#;
        let (_t, st, diags) = crate::typecheck_str_opts(src, &library_opts(&jar));
        assert!(!crate::has_errors(&diags));
        let supplied = st
            .get(st.list_sym)
            .members
            .iter()
            .filter(|&&m| st.get(m).jvm_name.starts_with('('))
            .count();
        assert_eq!(supplied, 0, "read a pickle for a member the prelude has");
    }

    #[test]
    fn descriptor_arity() {
        assert_eq!(desc_arity("()V"), Some(0));
        assert_eq!(
            desc_arity("(Ljava/lang/String;)Ljava/lang/String;"),
            Some(1)
        );
        assert_eq!(
            desc_arity("(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;"),
            Some(2)
        );
        assert_eq!(desc_arity("(I[[Ljava/lang/String;J)V"), Some(3));
        assert_eq!(desc_arity("no"), None);
    }
}
