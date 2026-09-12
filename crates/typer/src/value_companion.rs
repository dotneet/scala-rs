//! nsc's `extmethods`, as far as the *pickle* is concerned.
//!
//! A value class's methods are compiled to `name$extension` methods that take
//! the underlying value instead of a receiver, and nsc declares them on the
//! class's **companion module**, synthesizing that module when the source did
//! not write one. `extmethods` runs before `pickler`, so those declarations
//! are part of the signature every later compilation reads:
//! `ExtensionMethods.extensionMethod` looks the method up in
//! `imeth.owner.companionModule.info` and asserts if it is not there.
//!
//! We keep the `$extension` code as statics on the value class (that is what
//! `gen::invoke_value_extension` calls inside one run), and `gen.rs` writes a
//! companion classfile whose methods forward to them. This pass supplies the
//! matching *symbols*, so the pickle says what the classfile does. Without it,
//! real scalac compiling
//!
//! ```text
//! class Ops(val x: Int) extends AnyVal { def inc: Int = x + 1 }   // scala-rs
//! new myp.Ops(41).inc                                            // scalac
//! ```
//!
//! died at its own erasure phase with
//! `AssertionError: no extension method found for: method inc:Int`.
//!
//! Two details of `ExtensionMethods.normalize` fix the shape of what is
//! declared, and both are load-bearing:
//!
//! * the receiver parameter must be named `$this` (`nme.SELF`) -- normalize
//!   recognises the receiver by *name*, not by position;
//! * the method's type parameters are the method's own followed by the
//!   **class's**, because normalize is
//!   `tparams dropRight clazz.typeParams.length` with
//!   `restpe.substSym(tparams takeRight clazz.typeParams.length,
//!   clazz.typeParams)` -- the class's come last. `javap -p -s` on scalac
//!   2.13.16 agrees: `final class ApOps[F[_], A] extends AnyVal { def
//!   map2[B, C](...) }` gets `public final <B, C, F, A> F map2$extension(...)`.
//!
//!   Declaring them the other way round did not fail to compile and did not
//!   fail to run; it made real scalac *crash* on any client that called such a
//!   method, because normalize then read the parameter types against the wrong
//!   parameters: `no extension method found for: method map2:[B, C](fb: F[B])
//!   (f: (A, B) => C)(implicit F: Ap[F]): F[C]` with the candidate normalized
//!   to `map2$extension:[F[_], A](fb: F[F], f: (A, F) => A, F: Ap[F]): F[A]`.
//!   That is cats' `ApplyOps.map2`, i.e. `(fa, fb).mapN(f)` -- the single most
//!   used piece of cats syntax there is.
//!
//! Runs after the whole run is typed and before `pickle_all`, so nothing it
//! adds can change how anything resolves.

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};

use crate::erasure::for_each_child;

use crate::symbol::{SymKind, SymbolTable};

/// Declare `name$extension` on every source value class's companion module,
/// creating the companion when the source did not write one.
pub fn add_value_class_companions(tree: &Tree, st: &mut SymbolTable) {
    let mut classes = Vec::new();
    collect_value_classes(tree, st, &mut classes);
    for (cls, methods) in classes {
        let comp = companion_module_class(cls, st);
        for m in methods {
            declare_extension(st, comp, cls, m);
        }
    }
}

/// Every value class this unit defines, with the body methods that get an
/// `$extension`. The filter mirrors `gen::emit_value_extension` exactly: a
/// `def` with a body, constructors excluded.
fn collect_value_classes(tree: &Tree, st: &SymbolTable, out: &mut Vec<(SymbolId, Vec<SymbolId>)>) {
    if let TreeKind::ClassDef { impl_, .. } = &tree.kind {
        // Only where `gen::walk_stats` emits the companion classfile: a value
        // class is top-level or a member of an object (SLS 3.2.10), and
        // declaring one for anything else would put a class in the pickle
        // that no classfile answers to.
        let owner_ok = !tree.sym.is_none()
            && matches!(
                st.get(st.get(tree.sym).owner).kind,
                SymKind::Package | SymKind::ModuleClass | SymKind::NoSymbol
            );
        if owner_ok && st.is_value_class(tree.sym) {
            let mut methods = Vec::new();
            for stt in &impl_.body {
                let TreeKind::DefDef { name, rhs, .. } = &stt.kind else {
                    continue;
                };
                if rhs.is_empty() || stt.sym.is_none() || name == "<init>" || name == "<clinit>" {
                    continue;
                }
                methods.push(stt.sym);
            }
            out.push((tree.sym, methods));
        }
    }
    for_each_child(tree, &mut |c| collect_value_classes(c, st, out));
}

/// The module class of `cls`'s companion, synthesized if the source wrote
/// none. `SymbolTable::alloc` registers the new module in its owner's members,
/// which is where `SymbolTable::companion_module` looks for it.
fn companion_module_class(cls: SymbolId, st: &mut SymbolTable) -> SymbolId {
    if let Some(m) = st.companion_module(cls) {
        return st.module_class_of(m);
    }
    let owner = st.get(cls).owner;
    let name = st.get(cls).name.clone();
    let jvm = format!("{}$", st.get(cls).jvm_name);
    let mcls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL).with(Flags::SYNTHETIC),
        &jvm,
    );
    let m = st.alloc(
        &name,
        owner,
        SymKind::Module,
        Flags::MODULE.with(Flags::SYNTHETIC),
        &jvm,
    );
    st.get_mut(m).ty = Type::ModuleRef(mcls);
    st.get_mut(mcls).ty = Type::ModuleRef(mcls);
    mcls
}

/// `def name$extension[m's tparams, C's tparams]($this: C[...], m's params): R`
/// -- the class's last, as `normalize` reads them (see the module comment).
fn declare_extension(st: &mut SymbolTable, comp: SymbolId, cls: SymbolId, meth: SymbolId) {
    let name = format!("{}$extension", st.get(meth).name);
    let ret = match &st.get(meth).ty {
        Type::Method { ret, .. } => (**ret).clone(),
        t => t.clone(),
    };
    // The *clause structure* is part of the signature nsc matches against.
    // `normalize` only strips the `$this` clause, so `def map2[B, C](fb: F[B])
    // (f: (A, B) => C)(implicit F: Ap[F])` has to stay three clauses after the
    // receiver: flattened into one, nsc normalized the candidate to
    // `map2$extension:[B, C](fb, f, F)` and still reported `no extension method
    // found` for the three-clause original.
    let param_tys: Vec<Type> = match &st.get(meth).ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        _ => Vec::new(),
    };
    // `uncurry` has already joined the source's clauses into one on the
    // symbol's type and recorded their arities in `pickle_clauses`; the pickler
    // reads that record back (`pickle::clause_sizes`). The extension method
    // needs the same record with its receiver clause in front.
    let meth_clauses = st.get(meth).pickle_clauses.clone();
    let ext_clauses: Vec<usize> = if meth_clauses.iter().sum::<usize>() == param_tys.len() {
        std::iter::once(1).chain(meth_clauses).collect()
    } else {
        Vec::new()
    };
    let params = st.get(meth).params.clone();
    // A signature the pickler would have to guess at is worse than none: skip
    // it rather than declare a member whose type disagrees with the classfile.
    if params.len() != param_tys.len() {
        return;
    }
    let cls_tparams = st.get(cls).tparams.clone();
    let self_ty = Type::Class {
        sym: cls,
        args: cls_tparams
            .iter()
            .map(|&t| Type::TypeParam(t))
            .collect::<Vec<_>>(),
    };
    let ext = st.alloc(
        &name,
        comp,
        SymKind::Method,
        Flags::FINAL.with(Flags::SYNTHETIC),
        "",
    );
    // `nme.SELF`: `ExtensionMethods.normalize` finds the receiver by this name.
    let this = st.alloc("$this", ext, SymKind::Term, Flags::PARAM, "");
    st.get_mut(this).ty = self_ty.clone();
    let mut all_params = vec![this];
    all_params.extend(params);
    let mut all_tys = vec![self_ty];
    all_tys.extend(param_tys);
    let mut tparams = st.get(meth).tparams.clone();
    tparams.extend(cls_tparams);
    st.get_mut(ext).params = all_params.clone();
    st.get_mut(ext).paramss = vec![all_params];
    st.get_mut(ext).tparams = tparams;
    st.get_mut(ext).pickle_clauses = ext_clauses;
    st.get_mut(ext).ty = Type::Method {
        paramss: vec![all_tys],
        ret: Box::new(ret),
    };
}
