//! Install symbols recovered from classpath classfiles / ScalaSignature pickles.

use scala_rs_parser::{Flags, SymbolId, Type};
use std::collections::HashMap;

use crate::check::{ClasspathClass, ClasspathMethod, ClasspathType, ClasspathTypeParam};
use crate::symbol::{SymKind, SymbolTable};

mod abi;
mod descriptor;

pub use abi::adapt_classpath;

pub fn install_classpath(st: &mut SymbolTable, classes: &[ClasspathClass]) {
    let mut installed: Vec<(usize, SymbolId)> = Vec::new();
    // Nested classfiles (`enrich/package$Rich`) must see their outer module
    // (`enrich/package$`) first, otherwise they land as `package$Rich` on the
    // package and `import enrich._` cannot convert via `Rich`.
    let mut order: Vec<usize> = (0..classes.len()).collect();
    order.sort_by_key(|&i| nest_depth(&classes[i].jvm_name));
    for i in order {
        let c = &classes[i];
        if c.jvm_name.contains("$anon") || c.jvm_name.ends_with("$class") {
            continue;
        }
        if is_forwarder_of_module(classes, c) {
            continue;
        }
        // Pure Java classfiles have no ScalaSignature. Installing them here
        // (root owner, no JAVA/PROTECTED/STATIC) shadows on-demand completion
        // via `install_java_class` and drops JLS flags. The Java loader on
        // `binary_path` completes them instead.
        if c.pickle.is_none() {
            continue;
        }
        let (owner, simple) = classpath_symbol_owner(st, &c.jvm_name);
        if simple.is_empty() {
            continue;
        }
        if c.is_module {
            let jvm = if c.jvm_name.ends_with('$') {
                c.jvm_name.clone()
            } else {
                format!("{}$", c.jvm_name)
            };
            let existing = st
                .lookup_member(owner, &simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Module)
                .or_else(|| {
                    if owner == st.root {
                        st.lookup(&simple)
                            .into_iter()
                            .find(|&s| st.get(s).kind == SymKind::Module)
                    } else {
                        None
                    }
                });
            if let Some(m) = existing {
                installed.push((i, st.module_class_of(m)));
                continue;
            }
            let cls = st.alloc(
                &format!("{simple}$"),
                owner,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                &jvm,
            );
            let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, &jvm);
            st.get_mut(m).ty = Type::ModuleRef(cls);
            st.get_mut(cls).ty = Type::ModuleRef(cls);
            if owner == st.root {
                st.enter_in_current(&simple, m);
            }
            installed.push((i, cls));
        } else {
            let existing = st
                .lookup_member(owner, &simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Class)
                .or_else(|| {
                    if owner == st.root {
                        st.lookup(&simple)
                            .into_iter()
                            .find(|&s| st.get(s).kind == SymKind::Class)
                    } else {
                        None
                    }
                });
            if let Some(id) = existing {
                installed.push((i, id));
                continue;
            }
            // A Scala trait compiles to an interface. Without the flag the
            // backend emits `invokevirtual` against it and the JVM answers
            // with `IncompatibleClassChangeError` at the first call.
            let flags = if c.is_interface {
                Flags::INTERFACE
            } else {
                Flags::EMPTY
            };
            let id = st.alloc(&simple, owner, SymKind::Class, flags, &c.jvm_name);
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            st.get_mut(id).parents = vec![Type::AnyRef];
            if owner == st.root {
                st.enter_in_current(&simple, id);
            }
            installed.push((i, id));
        }
    }

    // Install all declaration shells before connecting parents. This keeps
    // class type parameters and direct abstract members visible to the
    // hierarchy pass; their bounds are resolved only after the complete graph
    // exists.
    for (i, owner) in &installed {
        let (i, owner) = (*i, *owner);
        let c = &classes[i];
        install_tparams(st, owner, &c.pickle_tparams);
        declare_type_members(st, owner, &c.type_members);
    }

    attach_classpath_parents(st, classes, &installed);

    for (i, owner) in &installed {
        let (i, owner) = (*i, *owner);
        let c = &classes[i];
        if owner.0 >= st.prelude_end {
            st.pending_classpath_signatures.insert(owner);
        }
        resolve_type_member_bounds(st, owner, &c.type_members);
        let accessors = nullary_accessors(c);
        if let Some(p) = &c.pickle {
            for m in p {
                if has_member(st, owner, &m.name) {
                    continue;
                }
                if m.is_val {
                    let mut ty = resolve_type_in(st, owner, &m.ret, &[]);
                    // The pickle subset keeps member types as *simple* names,
                    // so a val whose type lives in another package comes back
                    // unresolved. The getter's descriptor in the class file
                    // names the same type in full, so fall back to it rather
                    // than install a member nothing can be selected from.
                    if matches!(ty, Type::Named { .. }) {
                        if let Some(g) = c
                            .methods
                            .iter()
                            .find(|g| g.name == m.name && g.desc.starts_with("()"))
                        {
                            let (_, ret) = parse_method_desc(st, &g.desc);
                            if !matches!(ret, Type::Named { .. }) {
                                ty = ret;
                            }
                        }
                    }
                    let id = add_term(st, owner, &m.name, ty);
                    if m.is_implicit {
                        st.get_mut(id).flags = st.get(id).flags.with(Flags::IMPLICIT);
                    }
                    // `MUTABLE` and `DEFERRED` live only in the pickle: an
                    // interface declares a `val`'s and a `var`'s getter
                    // identically, and declares a concrete member of a trait
                    // exactly as it declares a deferred one.
                    if m.is_mutable {
                        let f = st.get(id).flags.with(Flags::MUTABLE);
                        st.get_mut(id).flags = f;
                    }
                    st.get_mut(id).deferred_val = m.is_deferred;
                    mark_via_accessor(st, id, &m.name, &accessors);
                    continue;
                }
                if m.is_ctor {
                    install_ctor(st, owner, m, &accessors, &c.fields);
                    continue;
                }
                let fallback_names =
                    method_type_name_fallbacks(st, c, &m.name, m.param_types.len());
                let id = add_method(
                    st,
                    owner,
                    &m.name,
                    m.param_names.clone(),
                    m.param_types.clone(),
                    m.ret.clone(),
                    m.tparams.clone(),
                    m.clause_sizes.clone(),
                    &fallback_names,
                );
                if m.is_implicit {
                    let f = st.get(id).flags.with(Flags::IMPLICIT);
                    st.get_mut(id).flags = f;
                }
                if m.is_deferred {
                    let f = st.get(id).flags.with(Flags::ABSTRACT);
                    st.get_mut(id).flags = f;
                }
            }
        }
        for m in &c.methods {
            if has_member(st, owner, &m.name) {
                continue;
            }
            if m.name == "<init>" && !st.get(owner).ctor_fields.is_empty() {
                continue;
            }
            let (params, ret) = parse_classpath_method(st, owner, m);
            let names: Vec<String> = (0..params.len()).map(|i| format!("x${i}")).collect();
            let id = add_method_erased(st, owner, &m.name, names, params, ret);
            // Non-public methods can be absent from the pickle API subset.
            // Their classfile fallback still has real access restrictions;
            // treating it as public allowed calls that fail on the JVM.
            if m.access & 0x0002 != 0 {
                st.get_mut(id).flags = st.get(id).flags.with(Flags::PRIVATE);
            }
            if m.access & 0x0004 != 0 {
                st.get_mut(id).flags = st.get(id).flags.with(Flags::PROTECTED);
            }
        }
        mark_defaults_from_getters(st, owner);
        copy_package_object_members(st, owner, c);
        // This eager `-cp` scan only ever sets `Flags::INTERFACE` (never
        // `Flags::TRAIT`; both are checked together everywhere that matters),
        // and a trait's pickle subset never carries an `m.is_ctor` entry --
        // see `ensure_interface_ctor`'s doc comment for why. Without this, a
        // trait reached through a loose classfile directory rather than a jar
        // (this scan does not read jars at all; `-cp somedir` does) got no
        // `<init>` from any path.
        ensure_interface_ctor(st, owner);
        // The eager ScalaSignature subset does not carry accessor visibility.
        // Its term slot corresponds to the actual nullary JVM getter. Match
        // that getter, not every method in a same-named overload family.
        for method in &c.methods {
            if !method.desc.starts_with("()") {
                continue;
            }
            for id in st.lookup_member(owner, &method.name) {
                if st.get(id).owner != owner || st.get(id).kind != SymKind::Term {
                    continue;
                }
                if method.access & 0x0002 != 0 {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::PRIVATE);
                }
                if method.access & 0x0004 != 0 {
                    st.get_mut(id).flags = st.get(id).flags.with(Flags::PROTECTED);
                }
            }
        }
    }

    // Everything after this point is this run's own source (or a pickle
    // member supplied on demand). `class_rules::unapplied_new_error` judges
    // only those: a constructor read from a classfile has no implicit marker,
    // so `new UnrolledBuffer[Int]` -- whose clause is an implicit `ClassTag` --
    // looked like a call missing its argument.
    st.source_start = st.symbols.len() as u32;
}

/// Give each `-cp` class the parents its classfile header names.
///
/// The pickle subset records member types by *simple* name and drops the
/// inheritance graph entirely, so without this a class from `-cp` looks like it
/// extends nothing: `t.greet()` on a `trait T extends Base` is "not a member",
/// and a member that *is* found is attributed to the wrong owner. The classfile
/// header is where `super_class`/`interfaces` survive intact.
///
/// Run after every class symbol is installed, so a parent that comes later in
/// the scan is already there, and before member decoding so inherited type
/// members are available while method signatures are resolved. `has_member`
/// remains owner-local and therefore does not let these early parent links hide
/// a class's own declarations.
///
/// Type arguments are not recoverable from the header (`extends Base[Int]` is
/// just `Base` there), so a parent is attached without them. That is why an
/// existing symbol whose parents are already known -- the prelude's, or the
/// Java loader's, both of which carry arguments -- is left alone.
fn attach_classpath_parents(
    st: &mut SymbolTable,
    classes: &[ClasspathClass],
    installed: &[(usize, SymbolId)],
) {
    let mut by_jvm: std::collections::HashMap<String, SymbolId> = std::collections::HashMap::new();
    for s in st.symbols.iter() {
        if s.is_class_like() && !s.jvm_name.is_empty() {
            by_jvm.entry(s.jvm_name.clone()).or_insert(s.id);
        }
    }
    for (i, owner) in installed {
        let c = &classes[*i];
        let owner = *owner;
        // `extends AnyVal` survives only in the pickle: a value class's class
        // file has `java/lang/Object` for a superclass and no interfaces, so
        // the header below can never produce it. It is appended rather than
        // substituted for the head, so a value class that also extends a
        // universal trait keeps that trait. The *library* is left alone,
        // prelude symbol or not: the prelude models scala-library's value
        // classes by hand, and the ones it models as ordinary classes it
        // models that way on purpose (see `pickle_supply::attach_parents`).
        let add_anyval =
            c.extends_anyval && owner.0 >= st.prelude_end && !c.jvm_name.starts_with("scala/") && {
                !st.get(owner).parents.iter().any(|p| {
                    matches!(p, Type::AnyVal)
                        || st.class_sym_of(p).is_some_and(|s| s == st.anyval_sym)
                })
            };
        // Only when nothing better is known. `parents` is `[AnyRef]` for a
        // class this module just created and richer for one that was already
        // declared elsewhere.
        if st
            .get(owner)
            .parents
            .iter()
            .any(|p| !matches!(p, Type::AnyRef))
        {
            if add_anyval {
                st.get_mut(owner).parents.push(Type::AnyVal);
            }
            continue;
        }
        let mut ps = vec![Type::AnyRef];
        for n in c.super_name.iter().chain(c.interfaces.iter()) {
            if n == "java/lang/Object" {
                continue;
            }
            let Some(&pid) = by_jvm.get(n.as_str()) else {
                continue;
            };
            if pid == owner || pid == st.object_sym {
                continue;
            }
            let ty = Type::Class {
                sym: pid,
                args: vec![],
            };
            if !ps.iter().any(|p| same_class(p, &ty)) {
                ps.push(ty);
            }
        }
        if add_anyval {
            ps.push(Type::AnyVal);
        }
        if ps.len() > 1 {
            st.get_mut(owner).parents = ps;
        }
    }
}

fn has_member(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    // Parent links are attached before member decoding so inherited type
    // members can resolve method signatures. Only the owner's own method/term
    // declarations suppress another eager installation; using lookup_member
    // here would make an inherited `n` hide a class's own `n`.
    st.get(owner).members.iter().any(|&id| {
        st.get(id).owner == owner
            && matches!(st.get(id).kind, SymKind::Method | SymKind::Term)
            && st.get(id).name == name
    })
}

fn install_tparams(st: &mut SymbolTable, owner: SymbolId, tps: &[ClasspathTypeParam]) {
    if tps.is_empty() || !st.get(owner).tparams.is_empty() {
        return;
    }
    let ids = alloc_tparams(st, owner, tps);
    st.get_mut(owner).tparams = ids;
}

/// Install the type declarations a ScalaSignature carries but the JVM class
/// file cannot. They must be present before methods are decoded: an
/// unqualified `type T` in an inherited API otherwise becomes `Type::Named`
/// and cannot match a path-dependent `p.T` at a call site.
fn declare_type_members(
    st: &mut SymbolTable,
    owner: SymbolId,
    members: &[crate::check::ClasspathTypeMember],
) {
    for member in members {
        // Concrete aliases are completed by PickleSupply, which preserves the
        // declaring path and inherited override information. Eagerly adding
        // the compact classpath spelling here would shadow that richer view
        // and break aliases used as binary parent constructors (`Owner.Alias`)
        // before their receiver is typed. Abstract members have no RHS for
        // PickleSupply to reduce and are the declarations this eager path must
        // retain (for example `BasicProfile.SchemaDescription`).
        if member.alias.is_some() {
            continue;
        }
        if st.get(owner).members.iter().any(|&id| {
            st.get(id).owner == owner
                && st.get(id).kind == SymKind::TypeMember
                && st.get(id).name == member.name
        }) {
            continue;
        }
        let id = st.alloc(&member.name, owner, SymKind::TypeMember, Flags::EMPTY, "");
        let tparams = alloc_tparams(st, id, &member.tparams);
        st.get_mut(id).tparams = tparams.clone();
        // Abstract members remain opaque until a concrete subclass fixes them.
        st.get_mut(id).ty = Type::TypeMember(id);
        st.get_mut(owner).members.push(id);
    }
}

/// Resolve bounds after every classpath class has its parent links. A bound
/// can name an inherited class/member, so doing this during per-class
/// installation makes the answer depend on classpath iteration order.
fn resolve_type_member_bounds(
    st: &mut SymbolTable,
    owner: SymbolId,
    members: &[crate::check::ClasspathTypeMember],
) {
    for member in members {
        if member.alias.is_some() {
            continue;
        }
        let Some(id) = st.get(owner).members.iter().copied().find(|&id| {
            st.get(id).owner == owner
                && st.get(id).kind == SymKind::TypeMember
                && st.get(id).name == member.name
        }) else {
            continue;
        };
        let tparams = st.get(id).tparams.clone();
        let lower = resolve_type_in(st, owner, &member.lower_bound, &tparams);
        let upper = resolve_type_in(st, owner, &member.upper_bound, &tparams);
        st.get_mut(id).bound_lo = Some(lower);
        st.get_mut(id).bound_hi = Some(upper);
    }
}

/// Allocate type parameter symbols, keeping each one's own parameters. Without
/// them a `F[_]` read from a classfile looked like a proper type and every
/// `Applicative[F]` failed the kind check.
fn alloc_tparams(
    st: &mut SymbolTable,
    owner: SymbolId,
    tps: &[ClasspathTypeParam],
) -> Vec<SymbolId> {
    let mut ids = Vec::new();
    for tp in tps {
        let id = st.alloc(&tp.name, owner, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(id).ty = Type::TypeParam(id);
        if !tp.tparams.is_empty() {
            let nested = alloc_tparams(st, id, &tp.tparams);
            st.get_mut(id).tparams = nested;
        }
        ids.push(id);
    }
    ids
}

fn install_java_tparams(st: &mut SymbolTable, owner: SymbolId, params: &[crate::javasign::JParam]) {
    if params.is_empty() || !st.get(owner).tparams.is_empty() {
        return;
    }
    // A JVM generic signature cannot express a higher kind, so these are all
    // proper types.
    let names: Vec<ClasspathTypeParam> = params
        .iter()
        .map(|p| ClasspathTypeParam::simple(p.name.clone()))
        .collect();
    install_tparams(st, owner, &names);
    let env = tparam_env(st, owner);
    let ids = st.get(owner).tparams.clone();
    for (p, id) in params.iter().zip(ids) {
        let bounds: Vec<Type> = p
            .bounds
            .iter()
            .map(|b| jtype_to_type(st, b, &env))
            .filter(|t| !matches!(t, Type::Any | Type::AnyRef))
            .collect();
        st.get_mut(id).parents = bounds;
    }
}

fn add_term(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Term, Flags::EMPTY, "");
    st.get_mut(id).ty = ty;
    id
}

/// JVM names of the class file's argument-less methods -- the accessors.
///
/// scalac makes the field behind a `val` / `var` `private` and publishes a
/// getter of the same name beside it, so a member of a separately compiled
/// class has to be *called*, not read with `getfield` (see
/// [`crate::symbol::Symbol::via_accessor`]). The class file is the ground
/// truth about which members have one: a `private[this] val` does not, and a
/// constructor parameter that is not a `val` does not either, and both must
/// keep whatever they had.
fn nullary_accessors(c: &ClasspathClass) -> std::collections::HashSet<String> {
    c.methods
        .iter()
        .filter(|g| g.desc.starts_with("()") && g.name != "<init>")
        .map(|g| g.name.clone())
        .collect()
}

fn mark_via_accessor(
    st: &mut SymbolTable,
    id: SymbolId,
    name: &str,
    accessors: &std::collections::HashSet<String>,
) {
    if accessors.contains(&scala_rs_pickle::names::encode_method_name(name)) {
        st.get_mut(id).via_accessor = true;
    }
}

fn install_ctor(
    st: &mut SymbolTable,
    owner: SymbolId,
    m: &crate::check::ClasspathPickleMethod,
    accessors: &std::collections::HashSet<String>,
    jvm_fields: &[crate::check::ClasspathField],
) {
    let mut fields = Vec::new();
    for (i, (n, tn)) in m.param_names.iter().zip(m.param_types.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let ty = resolve_type_in(st, owner, tn, &[]);
        let existing = st
            .lookup_member(owner, &pname)
            .into_iter()
            .find(|&id| st.get(id).kind == SymKind::Term);
        let fid = if let Some(id) = existing {
            st.get_mut(id).ty = ty;
            id
        } else {
            add_term(st, owner, &pname, ty)
        };
        if m.param_flags
            .get(i)
            .is_some_and(|f| f & scala_rs_pickle::read::pflags::DEFAULTPARAM != 0)
        {
            let flags = st.get(fid).flags.with(Flags::DEFAULTPARAM);
            st.get_mut(fid).flags = flags;
        }
        mark_via_accessor(st, fid, &pname, accessors);
        // A constructor prototype alone is not a readable member. A real
        // public/protected instance field is readable even without a getter.
        if !accessors.contains(&scala_rs_pickle::names::encode_method_name(&pname)) {
            let exposed = jvm_fields.iter().find(|f| {
                f.name == scala_rs_pickle::names::encode_method_name(&pname)
                    && f.access & 0x0008 == 0
                    && f.access & (0x0001 | 0x0004) != 0
            });
            if let Some(field) = exposed {
                let sym = st.get_mut(fid);
                sym.flags.set(Flags::PRIVATE, false);
                sym.flags.set(Flags::LOCAL, false);
                sym.flags.set(Flags::PROTECTED, field.access & 0x0004 != 0);
            } else {
                st.get_mut(fid).flags = st.get(fid).flags.with(Flags::PRIVATE).with(Flags::LOCAL);
            }
        }
        fields.push(fid);
    }
    st.get_mut(owner).ctor_fields = fields.clone();
    add_method(
        st,
        owner,
        "<init>",
        m.param_names.clone(),
        m.param_types.clone(),
        ClasspathType::simple("Unit"),
        Vec::new(),
        m.clause_sizes.clone(),
        &HashMap::new(),
    );
}

/// Give a binary interface -- a Scala trait or a plain Java interface, read
/// from a classfile or a jar -- the empty-argument-list `<init>` that
/// `new I()` (and `extends I()`) resolve against, when nothing has already
/// given it a real one.
///
/// Neither a trait's class file nor its `ScalaSignature` pickle ever mentions
/// `<init>`: SLS 5.1.2 makes a trait's parents mere constraints that the
/// *mixing-in* class runs, never the trait itself, so nsc's own bytecode for
/// `Constraint` -- confirmed with `javap -p`, `interface … { … validate$(…);
/// … $init$(…); }` -- has no constructor at all, and neither does the pickle
/// (it lists `$init$` and both `validate` overloads, nothing named `<init>`).
/// Without this, `pick_ctor_at` finds no alternative at all for the class's
/// *own* `<init>` and reports "no matching overload for constructor
/// Constraint with arguments ()" for the one call every trait actually
/// accepts.
///
/// Deliberately mirrors `check_namer::namer_class`, which allocates a
/// zero-field `<init>` unconditionally for every template compiled from
/// source, trait included -- so a jar-backed trait resolves `new I()` /
/// `new I(x)` exactly the way a source one already does: the former finds its
/// one alternative and matches, the latter finds it and correctly reports "no
/// matching overload" for the non-empty argument list.
///
/// The symbol is never given a JVM descriptor and is never picked as a real
/// superclass constructor: `parent_super_ctor` (`crates/backend/src/gen_desc.rs`)
/// refuses any candidate whose owner `is_interface_sym`, so codegen never
/// tries to `invokespecial` it. An anonymous subclass's own `<init>` calls the
/// real superclass (`Object`, or whatever the linearization supplies) and lets
/// `mixin_init_calls` invoke the interface's `$init$`, exactly as it does for
/// a trait compiled in this run.
///
/// Only ever adds: a class or trait that already has its own `<init>` --
/// from source, from a real classfile constructor, or from an earlier call
/// here -- is left untouched.
fn ensure_interface_ctor(st: &mut SymbolTable, id: SymbolId) {
    if id.is_none() || !st.get(id).flags.contains(Flags::INTERFACE) {
        return;
    }
    let has_own_ctor = st
        .get(id)
        .members
        .iter()
        .any(|&m| st.get(m).owner == id && st.get(m).name == "<init>");
    if has_own_ctor {
        return;
    }
    let ctor = st.alloc("<init>", id, SymKind::Method, Flags::CONSTRUCTOR, "");
    st.get_mut(ctor).ty = Type::Method {
        paramss: vec![],
        ret: Box::new(Type::Unit),
    };
}

fn resolve_type_in(
    st: &SymbolTable,
    owner: SymbolId,
    ty: &ClasspathType,
    method_tps: &[SymbolId],
) -> Type {
    let args: Vec<Type> = ty
        .args
        .iter()
        .map(|a| resolve_type_in(st, owner, a, method_tps))
        .collect();
    let name = ty.name.as_str();
    if name == "_" {
        return Type::Wildcard;
    }
    for id in method_tps.iter().chain(st.get(owner).tparams.iter()) {
        if st.get(*id).name == name {
            return apply_args(Type::TypeParam(*id), args);
        }
    }
    let mut cur = owner;
    let mut seen = std::collections::HashSet::new();
    while !cur.is_none() && seen.insert(cur.0) {
        if let Some(id) = st
            .lookup_member(cur, name)
            .into_iter()
            .find(|&s| st.get(s).is_class_like() || st.get(s).kind == SymKind::TypeMember)
        {
            return match st.get(id).kind {
                SymKind::Module | SymKind::ModuleClass => apply_args(Type::ModuleRef(id), args),
                // A pickle writes an unqualified reference to an enclosing
                // abstract type member (for example `SchemaDescription` in
                // RelationalAPI). Keep that member identity so it can match
                // the same inherited member on a path-dependent receiver;
                // aliases still expose their right-hand side here.
                SymKind::TypeMember => apply_args(st.type_member_as_seen(id), args),
                _ => Type::Class { sym: id, args },
            };
        }
        cur = st.get(cur).owner;
    }
    resolve_type_name_args(st, name, args)
}

/// Apply type arguments to a constructor that is not a class symbol. A bare
/// constructor stays as it is.
fn apply_args(ctor: Type, args: Vec<Type>) -> Type {
    if args.is_empty() {
        return ctor;
    }
    Type::Applied {
        ctor: Box::new(ctor),
        args,
    }
}

fn add_method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    param_type_names: Vec<ClasspathType>,
    ret_name: ClasspathType,
    tparams: Vec<ClasspathTypeParam>,
    clause_sizes: Vec<usize>,
    fallback_names: &HashMap<String, Type>,
) -> SymbolId {
    let flags = if name.contains("$default$") {
        Flags::SYNTHETIC
    } else if name == "<init>" {
        Flags::CONSTRUCTOR
    } else {
        Flags::EMPTY
    };
    let id = st.alloc(name, owner, SymKind::Method, flags, "");
    let tp_ids = alloc_tparams(st, id, &tparams);
    st.get_mut(id).tparams = tp_ids.clone();
    let params: Vec<Type> = param_type_names
        .iter()
        .map(|n| {
            replace_named_with_fallback(resolve_type_in(st, owner, n, &tp_ids), fallback_names)
        })
        .collect();
    let ret = replace_named_with_fallback(
        resolve_type_in(st, owner, &ret_name, &tp_ids),
        fallback_names,
    );
    let mut pids = Vec::new();
    for (i, (n, ty)) in param_names.iter().zip(params.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let pid = st.alloc(&pname, id, SymKind::Term, Flags::PARAM, "");
        st.get_mut(pid).ty = ty.clone();
        pids.push(pid);
    }
    // The classfile descriptor has one flat parameter sequence, but the
    // ScalaSignature's nested MethodTypes retain the source clause shape.
    // Restore that shape here so a separately compiled `f(a)(b)` is not
    // exposed as the unrelated `f(a, b)`.
    let sizes = if !clause_sizes.is_empty() && clause_sizes.iter().sum::<usize>() == params.len() {
        clause_sizes
    } else if params.is_empty() {
        Vec::new()
    } else {
        vec![params.len()]
    };
    let mut paramss = Vec::with_capacity(sizes.len());
    let mut start = 0;
    for &size in &sizes {
        let end = start + size;
        paramss.push(pids[start..end].to_vec());
        start = end;
    }
    st.get_mut(id).params = pids;
    st.get_mut(id).paramss = paramss.clone();
    st.get_mut(id).ty = Type::Method {
        paramss: sizes
            .iter()
            .scan(0usize, |start, &size| {
                let end = *start + size;
                let clause = params[*start..end].to_vec();
                *start = end;
                Some(clause)
            })
            .collect(),
        ret: Box::new(ret),
    };
    id
}

fn add_method_erased(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    params: Vec<Type>,
    ret: Type,
) -> SymbolId {
    add_method_types(st, owner, name, param_names, params, ret)
}

/// Recover the fully qualified classes hidden by the pickle subset's simple
/// type names.  `ClasspathPickleMethod` intentionally keeps the compact names
/// used by the backend pickle reader (`Git`, `ObjectId`, ...), while the JVM
/// method descriptor and generic `Signature` retain their exact owners.  A
/// method taking a JGit class therefore used to be stored as `Type::Named`
/// and could not accept a value whose type came from the Java classpath.
///
/// Only unambiguous simple names are returned.  If one method mentions two
/// classes with the same leaf name, guessing would be worse than leaving the
/// name unresolved and letting the normal classpath path report it.
fn method_type_name_fallbacks(
    st: &mut SymbolTable,
    class: &ClasspathClass,
    name: &str,
    arity: usize,
) -> HashMap<String, Type> {
    let mut candidates: HashMap<String, Option<Type>> = HashMap::new();
    for method in class.methods.iter().filter(|m| m.name == name) {
        let (desc_params, _) = parse_method_desc(st, &method.desc);
        if desc_params.len() != arity {
            continue;
        }
        if let Some(signature) = method.signature.as_deref() {
            if let Some(parsed) = crate::javasign::parse_method_sig(signature) {
                for ty in parsed.params.iter().chain(std::iter::once(&parsed.ret)) {
                    collect_jtype_name_fallbacks(st, ty, &mut candidates);
                }
                continue;
            }
        }
        // A method without a generic Signature still gives exact names in its
        // descriptor. Pair those with the pickle's flat parameters.
        if let Some(pickled) = class.pickle.as_ref().and_then(|methods| {
            methods
                .iter()
                .find(|m| m.name == name && m.param_types.len() == arity)
        }) {
            for (pickled, descriptor) in pickled.param_types.iter().zip(desc_params) {
                if let Type::Class { .. } = descriptor {
                    record_type_name_fallback(&mut candidates, &pickled.name, descriptor);
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|(name, ty)| ty.map(|ty| (name, ty)))
        .collect()
}

fn collect_jtype_name_fallbacks(
    st: &mut SymbolTable,
    ty: &crate::javasign::JType,
    out: &mut HashMap<String, Option<Type>>,
) {
    use crate::javasign::JType;
    match ty {
        JType::Class { jvm, args } => {
            let simple = jvm.rsplit('/').next().unwrap_or(jvm);
            let mapped = if jvm == "java/lang/String" {
                Type::String
            } else if jvm == "java/lang/Object" {
                Type::Any
            } else {
                Type::Class {
                    sym: find_or_stub_java_class(st, jvm),
                    args: Vec::new(),
                }
            };
            record_type_name_fallback(out, simple, mapped);
            for arg in args {
                collect_jtype_name_fallbacks(st, arg, out);
            }
        }
        JType::Array(inner) | JType::Extends(inner) | JType::Super(inner) => {
            collect_jtype_name_fallbacks(st, inner, out)
        }
        JType::Void
        | JType::Boolean
        | JType::Byte
        | JType::Short
        | JType::Char
        | JType::Int
        | JType::Long
        | JType::Float
        | JType::Double
        | JType::Var(_)
        | JType::Star => {}
    }
}

fn record_type_name_fallback(out: &mut HashMap<String, Option<Type>>, name: &str, ty: Type) {
    if name.is_empty() {
        return;
    }
    match out.entry(name.to_string()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(ty));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().as_ref() != Some(&ty) {
                entry.insert(None);
            }
        }
    }
}

/// Replace only unresolved pickle names with exact classpath identities.  The
/// structural `FunctionN` representation is retained, but its argument types
/// are repaired recursively, which is needed for lambdas calling methods on a
/// Java parameter such as `CanonicalTreeParser`.
fn replace_named_with_fallback(ty: Type, names: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Named { name, args } => {
            let args = args
                .into_iter()
                .map(|arg| replace_named_with_fallback(arg, names))
                .collect::<Vec<_>>();
            if let Some(mapped) = names.get(&name) {
                match mapped.clone() {
                    Type::Class { sym, .. } => Type::Class { sym, args },
                    other if args.is_empty() => other,
                    _ => Type::Named { name, args },
                }
            } else {
                Type::Named { name, args }
            }
        }
        Type::Array(inner) => Type::Array(Box::new(replace_named_with_fallback(*inner, names))),
        Type::Tuple(items) => Type::Tuple(
            items
                .into_iter()
                .map(|item| replace_named_with_fallback(item, names))
                .collect(),
        ),
        Type::Function { params, ret } => Type::Function {
            params: params
                .into_iter()
                .map(|param| replace_named_with_fallback(param, names))
                .collect(),
            ret: Box::new(replace_named_with_fallback(*ret, names)),
        },
        Type::Class { sym, args } => Type::Class {
            sym,
            args: args
                .into_iter()
                .map(|arg| replace_named_with_fallback(arg, names))
                .collect(),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .into_iter()
                .map(|params| {
                    params
                        .into_iter()
                        .map(|param| replace_named_with_fallback(param, names))
                        .collect()
                })
                .collect(),
            ret: Box::new(replace_named_with_fallback(*ret, names)),
        },
        Type::ByName(inner) => Type::ByName(Box::new(replace_named_with_fallback(*inner, names))),
        Type::Repeated(inner) => {
            Type::Repeated(Box::new(replace_named_with_fallback(*inner, names)))
        }
        Type::Overload(items) => Type::Overload(
            items
                .into_iter()
                .map(|item| replace_named_with_fallback(item, names))
                .collect(),
        ),
        Type::Applied { ctor, args } => Type::Applied {
            ctor: Box::new(replace_named_with_fallback(*ctor, names)),
            args: args
                .into_iter()
                .map(|arg| replace_named_with_fallback(arg, names))
                .collect(),
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.map(|inner| Box::new(replace_named_with_fallback(*inner, names))),
            hi: hi.map(|inner| Box::new(replace_named_with_fallback(*inner, names))),
        },
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(replace_named_with_fallback(*prefix, names)),
            sym,
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(replace_named_with_fallback(*tpe, names)),
            annot,
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents
                .into_iter()
                .map(|parent| replace_named_with_fallback(parent, names))
                .collect(),
            decls,
        },
        other => other,
    }
}

fn is_forwarder_of_module(classes: &[ClasspathClass], c: &ClasspathClass) -> bool {
    if c.is_module || c.is_interface {
        return false;
    }
    // A case class `Point.class` sits next to companion `Point$.class`. That is
    // not a static forwarder: the pickle describes a real class (ctor / vals).
    if c.pickle
        .as_ref()
        .is_some_and(|p| p.iter().any(|m| m.is_ctor || m.is_val))
    {
        return false;
    }
    let dollar = format!("{}$", c.jvm_name);
    classes
        .iter()
        .any(|o| o.is_module && (o.jvm_name == dollar || o.jvm_name == c.jvm_name))
}

fn simple_name(jvm: &str) -> String {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    scala_rs_pickle::names::decode_method_name(last.trim_end_matches('$'))
}

fn nest_depth(jvm: &str) -> usize {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    let mut name = last.trim_end_matches('$');
    let mut depth = 0;
    while let Some(i) = scala_nesting_separator(name) {
        depth += 1;
        name = &name[..i];
    }
    depth
}

/// The `$` after a package object's class name is a nesting separator even
/// when the nested source name is `eq`: `package$eq$`. NameTransformer alone
/// reads `$eq` as the encoded `=` operator and therefore sees no nesting.
fn scala_nesting_separator(name: &str) -> Option<usize> {
    if let Some(rest) = name.strip_prefix("package$").filter(|s| !s.is_empty()) {
        return scala_rs_pickle::names::last_nesting_separator(rest)
            .map(|i| "package$".len() + i)
            .or(Some("package".len()));
    }
    scala_rs_pickle::names::last_nesting_separator(name)
}

fn classpath_nested_parent(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    let last_trim = last.trim_end_matches('$');
    let idx = scala_nesting_separator(last_trim)?;
    let outer_simple = &last_trim[..idx];
    let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let module_jvm = if pkg.is_empty() {
        format!("{outer_simple}$")
    } else {
        format!("{pkg}/{outer_simple}$")
    };
    let class_jvm = if pkg.is_empty() {
        outer_simple.to_string()
    } else {
        format!("{pkg}/{outer_simple}")
    };
    find_by_jvm(st, &module_jvm).or_else(|| find_by_jvm(st, &class_jvm))
}

fn classpath_symbol_owner(st: &mut SymbolTable, jvm_name: &str) -> (SymbolId, String) {
    if nest_depth(jvm_name) > 0 {
        let simple = scala_simple_name(jvm_name);
        if let Some(parent) = classpath_nested_parent(st, jvm_name) {
            return (parent, simple);
        }
        return (classpath_owner(st, jvm_name), simple);
    }
    (classpath_owner(st, jvm_name), simple_name(jvm_name))
}

fn classpath_owner(st: &mut SymbolTable, jvm_name: &str) -> SymbolId {
    let pkg = jvm_name.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    if pkg.is_empty() {
        st.root
    } else {
        ensure_package(st, pkg)
    }
}

fn copy_package_object_members(st: &mut SymbolTable, module_cls: SymbolId, c: &ClasspathClass) {
    if !c.is_module || simple_name(&c.jvm_name) != "package" {
        return;
    }
    let pkg = st.get(module_cls).owner;
    if pkg.is_none() {
        return;
    }
    let mems = st.get(module_cls).members.clone();
    for mem in mems {
        if !st.get(pkg).members.contains(&mem) {
            st.get_mut(pkg).members.push(mem);
        }
    }
}

fn add_method_types(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    params: Vec<Type>,
    ret: Type,
) -> SymbolId {
    let flags = if name.contains("$default$") {
        Flags::SYNTHETIC
    } else {
        Flags::EMPTY
    };
    let id = st.alloc(name, owner, SymKind::Method, flags, "");
    let mut pids = Vec::new();
    for (i, (n, ty)) in param_names.iter().zip(params.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let pid = st.alloc(&pname, id, SymKind::Term, Flags::PARAM, "");
        st.get_mut(pid).ty = ty.clone();
        pids.push(pid);
    }
    st.get_mut(id).params = pids.clone();
    st.get_mut(id).paramss = if pids.is_empty() { vec![] } else { vec![pids] };
    st.get_mut(id).ty = Type::Method {
        paramss: if params.is_empty() {
            vec![]
        } else {
            vec![params]
        },
        ret: Box::new(ret),
    };
    id
}

/// Mark the parameters a separately compiled method declares defaults for.
///
/// A class file records no per-parameter "has a default" bit. What it does
/// carry is the getter nsc emitted beside the method -- `apply$default$2`,
/// `halt$default$1` -- so the getters are the only evidence available for a
/// class whose pickle the typer does not read (or reads and then declines,
/// which is what happens to a case class's *synthetic* companion `apply`).
///
/// The getter's name does not say which *overload* it belongs to.
/// `org.scalatra.Control` declares `halt(ActionResult)` next to
/// `halt[T](Integer = null, T = (), Map = Map.empty)`, and taking the first
/// alternative would have given the one-parameter overload a default it does
/// not have. Arity settles most slots; where it does not, the getter's result
/// descriptor is the parameter's own declared type and settles the rest.
fn mark_defaults_from_getters(st: &mut SymbolTable, owner: SymbolId) {
    let members = st.get(owner).members.clone();
    let getters: Vec<(SymbolId, String, usize)> = members
        .iter()
        .filter(|&&id| st.get(id).kind == SymKind::Method)
        .filter_map(|&id| {
            let (meth, idx) = parse_default_getter(&st.get(id).name)?;
            Some((id, meth, idx))
        })
        .collect();
    for (gid, meth, idx) in getters {
        let cands: Vec<SymbolId> = st
            .lookup_member(owner, &meth)
            .into_iter()
            .filter(|&id| st.get(id).kind == SymKind::Method)
            .filter(|&id| st.get(id).params.len() >= idx)
            .collect();
        let mid = match cands.len() {
            0 => continue,
            1 => cands[0],
            _ => {
                let mut typed = cands
                    .iter()
                    .copied()
                    .filter(|&id| getter_fills_param(st, gid, id, idx));
                let Some(only) = typed.next() else {
                    continue;
                };
                // Two alternatives whose parameter has the same erased type:
                // nothing here can tell them apart, and guessing would give a
                // default to a method that has none.
                if typed.next().is_some() {
                    continue;
                }
                only
            }
        };
        let pid = st.get(mid).params[idx - 1];
        let f = st.get(pid).flags.with(Flags::DEFAULTPARAM);
        st.get_mut(pid).flags = f;
    }
}

/// Whether `gid`, a `name$default$idx` getter, returns what `mid`'s `idx`-th
/// parameter is declared as. Both descriptors come from the same class file,
/// so this is an exact comparison; when either is missing the answer is "no",
/// which leaves the ambiguity unresolved rather than resolving it wrongly.
fn getter_fills_param(st: &SymbolTable, gid: SymbolId, mid: SymbolId, idx: usize) -> bool {
    let gdesc = st.get(gid).jvm_name.clone();
    let mdesc = st.get(mid).jvm_name.clone();
    let (Some(gret), Some(mparams)) = (
        gdesc.split_once(')').map(|(_, r)| r.to_string()),
        crate::pickle_supply::desc_params(&mdesc),
    ) else {
        return false;
    };
    mparams.get(idx - 1).is_some_and(|p| *p == gret)
}

pub fn parse_default_getter(name: &str) -> Option<(String, usize)> {
    let i = name.rfind("$default$")?;
    let meth = &name[..i];
    let n: usize = name[i + "$default$".len()..].parse().ok()?;
    if meth.is_empty() || n == 0 {
        return None;
    }
    Some((meth.to_string(), n))
}

fn resolve_type_name(st: &SymbolTable, name: &str) -> Type {
    resolve_type_name_args(st, name, Vec::new())
}

fn resolve_type_name_args(st: &SymbolTable, name: &str, args: Vec<Type>) -> Type {
    // `FunctionN` / `TupleN` are structural in our `Type`, so they only become
    // themselves once their arguments are known.
    if let Some(n) = name.strip_prefix("Function") {
        if n.parse::<usize>().is_ok() && !args.is_empty() {
            let mut args = args;
            let ret = args.pop().unwrap_or(Type::Any);
            return Type::Function {
                params: args,
                ret: Box::new(ret),
            };
        }
    }
    if let Some(n) = name.strip_prefix("Tuple") {
        if n.parse::<usize>().is_ok() && args.len() > 1 {
            return Type::Tuple(args);
        }
    }
    if name == "Array" && args.len() == 1 {
        return Type::Array(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    if name == "<byname>" && args.len() == 1 {
        return Type::ByName(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    if name == "<repeated>" && args.len() == 1 {
        return Type::Repeated(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    match resolve_bare_type_name(st, name) {
        Type::Class { sym, args: old } if old.is_empty() && !args.is_empty() => {
            Type::Class { sym, args }
        }
        Type::Named { name, args: old } if old.is_empty() && !args.is_empty() => {
            Type::Named { name, args }
        }
        t => t,
    }
}

fn resolve_bare_type_name(st: &SymbolTable, name: &str) -> Type {
    match name {
        "Unit" | "V" => Type::Unit,
        "Boolean" | "Z" => Type::Boolean,
        "Int" | "I" => Type::Int,
        "Long" | "J" => Type::Long,
        "Float" | "F" => Type::Float,
        "Double" | "D" => Type::Double,
        "Char" | "C" => Type::Char,
        "String" => Type::String,
        "Object" | "Any" | "AnyRef" => Type::Any,
        // The two bottom types. Like `Int` and `String` above they are named
        // in the pickle by their simple name, and resolving them by lookup
        // instead found nothing and left a `Type::Named` behind: a separately
        // compiled `def n: Null` came out `()LNull;` and the call was a
        // `NoSuchMethodError` against nsc's `()Lscala/runtime/Null$;`.
        "Null" => Type::Null,
        "Nothing" => Type::Nothing,
        n if n.starts_with("Function") => Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Any),
        },
        n => {
            let found = st.lookup(n);
            if let Some(id) = found
                .iter()
                .copied()
                .find(|s| st.get(*s).kind == SymKind::Class)
            {
                Type::Class {
                    sym: id,
                    args: vec![],
                }
            } else if let Some(id) = found
                .iter()
                .copied()
                .find(|s| st.get(*s).is_class_like() || st.get(*s).kind == SymKind::Module)
            {
                match st.get(id).kind {
                    SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                    _ => Type::Class {
                        sym: id,
                        args: vec![],
                    },
                }
            } else {
                Type::Named {
                    name: n.to_string(),
                    args: vec![],
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescriptorSource {
    /// A ScalaSignature member whose names are the pickle's compact names.
    ScalaPickle,
    /// A descriptor emitted by scalac. Unit and the bottom types have Scala's
    /// erased runtime spellings in value positions.
    ScalaErased,
    /// A descriptor emitted by javac or another Java classfile producer.
    Java,
}

pub fn parse_method_desc(st: &mut SymbolTable, desc: &str) -> (Vec<Type>, Type) {
    parse_method_desc_with_source(st, desc, DescriptorSource::ScalaPickle)
}

fn parse_method_desc_with_source(
    st: &mut SymbolTable,
    desc: &str,
    source: DescriptorSource,
) -> (Vec<Type>, Type) {
    if let Some(method) = descriptor::method(desc) {
        let params = method
            .params
            .into_iter()
            .map(|field| descriptor_type(st, &field.ty, source))
            .collect();
        let ret = descriptor_type(st, &method.ret.ty, source);
        return (params, ret);
    }

    // Preserve the old reader's lenient fallback for a malformed or bare
    // descriptor: without a closing `)` it treated the entire suffix as the
    // result and exposed no parameters.
    let rest = desc.strip_prefix('(').unwrap_or(desc);
    (Vec::new(), parse_field_ty_with_source(st, rest, source).0)
}

/// Recover a classpath method's generic shape when its classfile provides a
/// JVM `Signature`; retain the erased descriptor as the fallback for methods
/// whose signature is absent or has method-local type parameters. The eager
/// classpath path otherwise loses the concrete type arguments on accessors
/// before `PickleSupply` can complete a separate consumer's receiver.
fn parse_classpath_method(
    st: &mut SymbolTable,
    owner: SymbolId,
    method: &ClasspathMethod,
) -> (Vec<Type>, Type) {
    let Some(sig) = method.signature.as_deref() else {
        return parse_method_desc(st, &method.desc);
    };
    let Some(parsed) = crate::javasign::parse_method_sig(sig) else {
        return parse_method_desc(st, &method.desc);
    };
    // `add_method_erased` has no method-type-parameter slot. Keep the old
    // descriptor path for that uncommon shape rather than installing a free
    // `Type::Var` or changing the broader classpath method representation.
    if !parsed.tparams.is_empty() {
        return parse_method_desc(st, &method.desc);
    }
    let env = tparam_env(st, owner);
    let params = parsed
        .params
        .iter()
        .map(|t| jtype_to_type(st, t, &env))
        .collect();
    let ret = java_result_obj(jtype_to_type(st, &parsed.ret, &env));
    (params, ret)
}

fn parse_field_ty_with_source(
    st: &mut SymbolTable,
    input: &str,
    source: DescriptorSource,
) -> (Type, usize) {
    if let Some(field) = descriptor::field(input) {
        let len = field.len;
        return (descriptor_type(st, &field.ty, source), len);
    }
    if input.as_bytes().first() == Some(&b'[') {
        // Keep the previous malformed-array behavior (`[Q` became
        // `Array[Any]`) even though the shared scanner rejects `Q`.
        let (inner, len) = parse_field_ty_with_source(st, &input[1..], source);
        return (Type::Array(Box::new(inner)), len + 1);
    }
    (Type::Any, usize::from(!input.is_empty()))
}

fn descriptor_type(
    st: &mut SymbolTable,
    raw: &descriptor::RawType<'_>,
    source: DescriptorSource,
) -> Type {
    match source {
        DescriptorSource::ScalaPickle => scala_pickle_descriptor_type(st, raw),
        DescriptorSource::ScalaErased => scala_erased_descriptor_type(st, raw),
        DescriptorSource::Java => java_descriptor_type(st, raw),
    }
}

fn primitive_descriptor_type(primitive: descriptor::Primitive) -> Type {
    use descriptor::Primitive;
    match primitive {
        Primitive::Void => Type::Unit,
        Primitive::Boolean => Type::Boolean,
        Primitive::Byte => Type::Byte,
        Primitive::Short => Type::Short,
        Primitive::Char => Type::Char,
        Primitive::Int => Type::Int,
        Primitive::Long => Type::Long,
        Primitive::Float => Type::Float,
        Primitive::Double => Type::Double,
    }
}

fn exact_descriptor_class(st: &mut SymbolTable, internal: &str) -> Type {
    Type::Class {
        sym: find_or_stub_java_class(st, internal),
        args: vec![],
    }
}

fn scala_pickle_descriptor_type(st: &mut SymbolTable, raw: &descriptor::RawType<'_>) -> Type {
    use descriptor::RawType;
    match raw {
        RawType::Primitive(primitive) => primitive_descriptor_type(*primitive),
        RawType::Array(inner) => Type::Array(Box::new(scala_pickle_descriptor_type(st, inner))),
        RawType::Object(object) => {
            let inner = object.as_str();
            let name = inner.rsplit('/').next().unwrap_or(inner);
            if inner == "java/lang/String" || name == "String" {
                return Type::String;
            }
            if inner == "java/lang/Object" {
                return Type::Any;
            }
            if inner == "scala/runtime/BoxedUnit" {
                // Unit erases to BoxedUnit in every Scala value position.
                return Type::Unit;
            }
            if inner == "scala/runtime/Nothing$" {
                return Type::Nothing;
            }
            if inner == "scala/runtime/Null$" {
                return Type::Null;
            }
            if name.starts_with("Function") {
                return Type::Function {
                    params: vec![Type::Any],
                    ret: Box::new(Type::Any),
                };
            }
            match resolve_type_name(st, name) {
                Type::Named { .. } => exact_descriptor_class(st, inner),
                ty => ty,
            }
        }
    }
}

fn scala_erased_descriptor_type(st: &mut SymbolTable, raw: &descriptor::RawType<'_>) -> Type {
    use descriptor::RawType;
    match raw {
        RawType::Primitive(primitive) => primitive_descriptor_type(*primitive),
        RawType::Array(inner) => Type::Array(Box::new(scala_erased_descriptor_type(st, inner))),
        RawType::Object(object) => {
            let inner = object.as_str();
            match inner {
                "java/lang/String" => Type::String,
                "java/lang/Object" => Type::Any,
                "scala/runtime/BoxedUnit" => Type::Unit,
                "scala/runtime/Nothing$" => Type::Nothing,
                "scala/runtime/Null$" => Type::Null,
                _ => exact_descriptor_class(st, inner),
            }
        }
    }
}

fn java_descriptor_type(st: &mut SymbolTable, raw: &descriptor::RawType<'_>) -> Type {
    use descriptor::RawType;
    match raw {
        RawType::Primitive(primitive) => primitive_descriptor_type(*primitive),
        RawType::Array(inner) => Type::Array(Box::new(java_descriptor_type(st, inner))),
        RawType::Object(object) => {
            let inner = object.as_str();
            match inner {
                "java/lang/String" => Type::String,
                "java/lang/Object" => Type::Any,
                "scala/runtime/Nothing$" => Type::Nothing,
                "scala/runtime/Null$" => Type::Null,
                // BoxedUnit is a real Java class when the descriptor comes
                // from javac, not Scala's erased representation of Unit.
                _ => exact_descriptor_class(st, inner),
            }
        }
    }
}

pub fn ensure_package(st: &mut SymbolTable, jvm: &str) -> SymbolId {
    if jvm.is_empty() {
        return st.root;
    }
    let mut cur = st.root;
    let mut sofar = String::new();
    for part in jvm.split('/') {
        if !sofar.is_empty() {
            sofar.push('/');
        }
        sofar.push_str(part);
        if let Some(id) = st
            .lookup_member(cur, part)
            .into_iter()
            .find(|&s| st.get(s).kind == SymKind::Package)
        {
            cur = id;
            continue;
        }
        if cur == st.root {
            if let Some(id) = st
                .lookup(part)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Package)
            {
                cur = id;
                continue;
            }
        }
        let id = st.alloc(part, cur, SymKind::Package, Flags::PACKAGE, &sofar);
        if cur == st.root {
            st.enter_in_current(part, id);
        }
        cur = id;
    }
    cur
}

pub fn install_java_class(st: &mut SymbolTable, c: &crate::javaclass::JavaClass) -> SymbolId {
    let owner = java_class_owner(st, &c.internal_name);
    install_java_class_in(st, c, owner)
}

pub fn install_java_class_in(
    st: &mut SymbolTable,
    c: &crate::javaclass::JavaClass,
    owner: SymbolId,
) -> SymbolId {
    let simple = if c.is_scala {
        scala_simple_name(&c.internal_name)
    } else {
        java_simple_name(&c.internal_name)
    };
    if simple.is_empty() {
        return find_or_stub_java_class(st, &c.internal_name);
    }
    if is_scala_module(c) {
        return install_java_module(st, c, owner);
    }
    if let Some(id) = st
        .lookup_member(owner, &simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Class)
    {
        apply_java_class_meta(st, id, c);
        fill_java_members(st, id, c);
        ensure_interface_ctor(st, id);
        return id;
    }
    if let Some(id) = find_by_jvm(st, &c.internal_name) {
        // An object-only Scala forwarder can be reached from a JVM descriptor
        // before its classfile is read. Its owner is then a plain JAVA class
        // stub. The ScalaSignature on `Foo.class` is authoritative, so promote
        // that placeholder into the module-class half instead of leaving two
        // symbols with one JVM name. Ordinary Scala classes (including nested
        // `Regex$Match.class`) must remain class symbols.
        if is_scala_module(c)
            && st.get(id).kind == SymKind::Class
            && st.get(id).flags.contains(Flags::JAVA)
            && !st.is_source_class(id)
        {
            st.get_mut(id).kind = SymKind::ModuleClass;
            st.get_mut(id).name = format!("{simple}$");
            st.get_mut(id).flags = Flags::MODULE.with(Flags::FINAL);
            st.get_mut(id).ty = Type::ModuleRef(id);
            let m = st.alloc(
                &simple,
                owner,
                SymKind::Module,
                Flags::MODULE,
                &c.internal_name,
            );
            st.get_mut(m).ty = Type::ModuleRef(id);
            if owner == st.root {
                st.enter_in_current(&simple, m);
            }
            apply_java_class_meta(st, id, c);
            fill_java_members(st, id, c);
            return id;
        }
        // A JVM descriptor can mention a Scala class before its classfile is
        // opened. At that point the generic Java-name fallback has no way to
        // distinguish nesting dollars from encoded operators: the return
        // type `Outer$$plus$plus` was stubbed as a class called `plus` under
        // another class called `plus`, rather than `Outer#++`. Once the Scala
        // classfile is loaded, both its decoded simple name and the owner that
        // requested the nested binary are authoritative. Repair only that
        // non-source Java placeholder; a real Java class and every source
        // symbol keep the identity they were declared with.
        if c.is_scala
            && st.get(id).kind == SymKind::Class
            && st.get(id).flags.contains(Flags::JAVA)
            && !st.is_source_class(id)
            && (st.get(id).owner != owner || st.get(id).name != simple)
        {
            let previous_owner = st.get(id).owner;
            if !previous_owner.is_none() {
                st.get_mut(previous_owner).members.retain(|&m| m != id);
            }
            st.get_mut(id).owner = owner;
            st.get_mut(id).name = simple.clone();
            if !owner.is_none() && !st.get(owner).members.contains(&id) {
                st.get_mut(owner).members.push(id);
            }
        }
        apply_java_class_meta(st, id, c);
        fill_java_members(st, id, c);
        enter_in_companion_scope(st, id, owner, &c.internal_name);
        ensure_interface_ctor(st, id);
        return id;
    }
    let flags = java_class_flags(c);
    let id = st.alloc(&simple, owner, SymKind::Class, flags, &c.internal_name);
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    if owner == st.root {
        st.enter_in_current(&simple, id);
    }
    apply_java_class_meta(st, id, c);
    fill_java_members(st, id, c);
    ensure_interface_ctor(st, id);
    id
}

/// A nested class file `Outer$Inner` does not say whether `Inner` was declared
/// by `class Outer` or by `object Outer`, and [`java_class_owner`] always
/// answers the *class*. Whichever spelling reaches the class file first is
/// therefore the only one that can see it: with
/// `cats/effect/kernel/Resource$ExitCase` entered while reading some other
/// jar class's member descriptors (`fs2/Stream` mentions it), the owner is the
/// trait `Resource`, and the source's `Resource.ExitCase` — a path through the
/// `Resource` **object** — looked `ExitCase` up on `Resource$`, found nothing,
/// and reported "type ExitCase is not a member of Resource$". Compiling
/// `slick/basic/BasicBackend.scala` on its own got the other order and worked,
/// which is what made the failure look like it needed the whole program.
///
/// So when the owner that *asked* is the companion module class of the owner
/// that has it, enter the same symbol in its scope too. No second symbol is
/// created and no owner is rewritten: both spellings simply reach the one
/// class there is.
fn enter_in_companion_scope(st: &mut SymbolTable, id: SymbolId, owner: SymbolId, internal: &str) {
    if owner.is_none() || st.get(owner).kind != SymKind::ModuleClass {
        return;
    }
    let held_by = st.get(id).owner;
    if held_by == owner {
        return;
    }
    let module_jvm = st.get(owner).jvm_name.clone();
    let Some(outer) = module_jvm.strip_suffix('$') else {
        return;
    };
    if outer.is_empty() || !internal.starts_with(&format!("{outer}$")) {
        return;
    }
    // Usually the first reader installed the nested class under the class
    // half of this companion. A scala-rs-written package object can instead
    // mention its nested modules while the package/module half is current,
    // so `held_by` is not necessarily that class. The JVM prefix above is the
    // stable evidence: `Outer$Inner` is reachable through both `class Outer`
    // and `object Outer`, irrespective of which route populated the table.
    if st.get(owner).members.contains(&id) {
        return;
    }
    st.get_mut(owner).members.push(id);
}

/// [`enter_in_companion_scope`] for a nested **object**, which needs both
/// halves: the module class and the module *term*, since a path
/// (`ExecutionContext.Implicits.global`) and an import selector both look the
/// term up.
///
/// `install_java_module` did not do this at all, so the defect
/// `enter_in_companion_scope` fixes for a nested class was still live for a
/// nested object. [`java_class_owner`] answers the *class*
/// `scala.concurrent.ExecutionContext` for
/// `scala/concurrent/ExecutionContext$Implicits$`, so whichever route reaches
/// that class file first decides where the object lives; when that route was
/// something reading the trait's descriptors, `import
/// scala.concurrent.ExecutionContext.Implicits.global` was "value Implicits is
/// not a member of ExecutionContext$" and every `Future { … }` behind it was a
/// missing `ExecutionContext`. On its own the same import compiles, which is
/// what made it look like it needed the whole program.
pub(crate) fn enter_module_in_companion_scope(
    st: &mut SymbolTable,
    cls: SymbolId,
    owner: SymbolId,
    internal: &str,
) {
    enter_in_companion_scope(st, cls, owner, internal);
    let held_by = st.get(cls).owner;
    if held_by.is_none() {
        return;
    }
    let term = st
        .get(held_by)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).kind == SymKind::Module && st.get(m).jvm_name == internal);
    if let Some(t) = term {
        enter_in_companion_scope(st, t, owner, internal);
    }
}

/// Make `id` -- a class file the table already holds -- reachable from the
/// owner that is asking for it now.
///
/// [`Checker::load_binary_into`] reads each class file once, and its
/// short-circuit used to answer "yes, it is loaded" without checking that the
/// caller's owner can see it. That is the same order-dependence
/// [`enter_in_companion_scope`] exists for, one level up: the first route in
/// decides the owner, and every later route is told the work is done.
pub(crate) fn enter_loaded_in_owner(st: &mut SymbolTable, id: SymbolId, owner: SymbolId) {
    let internal = st.get(id).jvm_name.clone();
    if internal.is_empty() {
        return;
    }
    match st.get(id).kind {
        SymKind::ModuleClass | SymKind::Module => {
            enter_module_in_companion_scope(st, id, owner, &internal)
        }
        SymKind::Class => enter_in_companion_scope(st, id, owner, &internal),
        _ => {}
    }
}

fn java_class_flags(c: &crate::javaclass::JavaClass) -> Flags {
    let mut flags = if c.is_scala {
        Flags::EMPTY
    } else {
        Flags::JAVA
    };
    if crate::javaclass::is_java_interface(c.access) {
        flags = flags.with(Flags::INTERFACE).with(Flags::ABSTRACT);
    }
    if crate::javaclass::is_java_enum(c.access) {
        flags = flags.with(Flags::ENUM);
    }
    if c.nested_static {
        flags = flags.with(Flags::STATIC);
    }
    flags
}

fn is_scala_module(c: &crate::javaclass::JavaClass) -> bool {
    c.scala_module || (c.internal_name.ends_with('$') && (c.has_module_field || c.is_scala))
}

fn install_java_module(
    st: &mut SymbolTable,
    c: &crate::javaclass::JavaClass,
    owner: SymbolId,
) -> SymbolId {
    let simple = scala_simple_name(&c.internal_name);
    // An object-only ScalaSignature lives on two JVM classes with different
    // jobs: `Foo.class` carries the pickle and static Java forwarders, while
    // `Foo$.class` is the actual module class and owns `MODULE$`.  The former
    // tells us that the source symbol is an object, but it must never become
    // the runtime identity of that object.
    let runtime_internal = if c.scala_module && !c.internal_name.ends_with('$') {
        format!("{}$", c.internal_name)
    } else {
        c.internal_name.clone()
    };
    if let Some(m) = st
        .lookup_member(owner, &simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Module)
    {
        let cls = st.module_class_of(m);
        apply_java_class_meta(st, cls, c);
        // Static forwarders are a Java-facing view, not source members of the
        // Scala module.  Its pickle (and, when loaded, `Foo$.class`) supplies
        // the instance declarations with the correct owner and flags.
        if !c.scala_module {
            fill_java_members(st, cls, c);
        }
        return cls;
    }
    if let Some(id) = find_by_jvm(st, &c.internal_name) {
        // A descriptor can stub `Foo` before its object-only pickle is read.
        // Promote that placeholder to the semantic module, but move its JVM
        // identity to the implementation class which actually owns MODULE$.
        if c.scala_module
            && st.get(id).kind == SymKind::Class
            && st.get(id).flags.contains(Flags::JAVA)
            && !st.is_source_class(id)
        {
            st.get_mut(id).kind = SymKind::ModuleClass;
            st.get_mut(id).name = format!("{simple}$");
            st.get_mut(id).flags = Flags::MODULE.with(Flags::FINAL);
            st.get_mut(id).ty = Type::ModuleRef(id);
            st.set_jvm_name(id, runtime_internal.clone());
            let m = st.alloc(
                &simple,
                owner,
                SymKind::Module,
                Flags::MODULE,
                &runtime_internal,
            );
            st.get_mut(m).ty = Type::ModuleRef(id);
            if owner == st.root {
                st.enter_in_current(&simple, m);
            }
            apply_java_class_meta(st, id, c);
            return id;
        }
        apply_java_class_meta(st, id, c);
        if !c.scala_module {
            fill_java_members(st, id, c);
        }
        enter_module_in_companion_scope(st, id, owner, &c.internal_name);
        return id;
    }
    let flags = Flags::MODULE.with(Flags::FINAL);
    let cls = st.alloc(
        &format!("{simple}$"),
        owner,
        SymKind::ModuleClass,
        flags,
        &runtime_internal,
    );
    let m = st.alloc(
        &simple,
        owner,
        SymKind::Module,
        Flags::MODULE,
        &runtime_internal,
    );
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    if owner == st.root {
        st.enter_in_current(&simple, m);
    }
    apply_java_class_meta(st, cls, c);
    if !c.scala_module {
        fill_java_members(st, cls, c);
    }
    cls
}

fn scala_simple_name(internal: &str) -> String {
    let name = internal
        .rsplit('/')
        .next()
        .unwrap_or(internal)
        .trim_end_matches('$');
    // In `package$eq$`, the first `$` is a nesting separator and `eq` is the
    // source identifier. NameTransformer alone cannot distinguish that from
    // a top-level `package=` object, because the encoded `=` chunk is also
    // `$eq`. A package object's JVM name fixes the boundary unambiguously.
    if let Some(simple) = name.strip_prefix("package$").filter(|s| !s.is_empty()) {
        let simple = scala_rs_pickle::names::last_nesting_separator(simple)
            .map_or(simple, |i| &simple[i + 1..]);
        return scala_rs_pickle::names::decode_method_name(simple);
    }
    let simple =
        scala_rs_pickle::names::last_nesting_separator(name).map_or(name, |i| &name[i + 1..]);
    scala_rs_pickle::names::decode_method_name(simple)
}

pub fn java_simple_name(internal: &str) -> String {
    let mut simple = internal.rsplit('/').next().unwrap_or(internal);
    if simple.ends_with('$') && simple.len() > 1 {
        simple = &simple[..simple.len() - 1];
    }
    if let Some(idx) = simple.rfind('$') {
        simple = &simple[idx + 1..];
    }
    simple.to_string()
}

pub(crate) fn java_class_owner(st: &mut SymbolTable, internal: &str) -> SymbolId {
    let trimmed = internal.trim_end_matches('$');
    if let Some((outer, _)) = trimmed.rsplit_once('$') {
        return find_or_stub_java_outer(st, outer);
    }
    let pkg = trimmed.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    ensure_package(st, pkg)
}

/// Resolve an enclosing JVM class while honoring Scala's two classfile views
/// of an object. A module-class symbol may retain either `Outer` (the
/// static-forwarder class carrying the ScalaSignature) or `Outer$` (the
/// implementation class), depending on which one was loaded first.
fn find_or_stub_java_outer(st: &mut SymbolTable, outer: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, outer) {
        return id;
    }
    if let Some(id) =
        find_by_jvm(st, &format!("{outer}$")).filter(|&id| st.get(id).kind == SymKind::ModuleClass)
    {
        return id;
    }
    find_or_stub_java_class(st, outer)
}

/// The symbol *for* a JVM class.
///
/// The primitive value classes are deliberately excluded even though they
/// carry a `java/lang/...` `jvm_name`: that name is the box `scala.Int` erases
/// to, not `scala.Int`'s identity. Returning `scala.Int` here made
/// `install_java_class_in` treat the real `java.lang.Integer` classfile as
/// "already installed", pour its members into `scala.Int` and never enter
/// `Integer` into `java.lang` — so `java.lang.Integer.valueOf(3)` failed with
/// "value Integer is not a member of <notype>". nsc keeps the two apart, and
/// so do we.
pub fn find_by_jvm(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    st.find_class_by_jvm(jvm)
}

pub fn find_or_stub_java_class(st: &mut SymbolTable, internal: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, internal) {
        return id;
    }
    let simple = java_simple_name(internal);
    let owner = java_class_owner(st, internal);
    stub_class_in(st, internal, simple, owner)
}

/// The signature reader has established that this is a Scala class. Encoded
/// operators are part of its simple name, not enclosing-class separators.
pub(crate) fn find_or_stub_scala_class(st: &mut SymbolTable, internal: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, internal) {
        return id;
    }
    let simple = scala_simple_name(internal);
    let trimmed = internal.trim_end_matches('$');
    let file_name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let owner = if let Some(i) = scala_nesting_separator(file_name) {
        let pkg = trimmed.rsplit_once('/').map_or("", |(p, _)| p);
        let outer = &file_name[..i];
        let outer = if pkg.is_empty() {
            outer.to_string()
        } else {
            format!("{pkg}/{outer}")
        };
        find_or_stub_scala_outer(st, &outer)
    } else {
        ensure_package(st, trimmed.rsplit_once('/').map_or("", |(p, _)| p))
    };
    stub_class_in(st, internal, simple, owner)
}

fn find_or_stub_scala_outer(st: &mut SymbolTable, outer: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, outer) {
        return id;
    }
    if let Some(id) =
        find_by_jvm(st, &format!("{outer}$")).filter(|&id| st.get(id).kind == SymKind::ModuleClass)
    {
        return id;
    }
    find_or_stub_scala_class(st, outer)
}

fn stub_class_in(
    st: &mut SymbolTable,
    internal: &str,
    simple: String,
    owner: SymbolId,
) -> SymbolId {
    // `cats/effect/kernel/Ref$` is the *companion*, not the trait. Stubbing it
    // as a `SymKind::Class` called `Ref` -- with the companion's name in
    // `jvm_name` -- made one symbol stand for two things: the object's members
    // landed on the trait, and the trait could never get a symbol of its own
    // (`ensure_class` declines a name whose `jvm_name` is not the key it
    // asked for), so `Ref#update` was read from the class file's generic
    // signature. That cannot write `F[Unit]`; it writes `TF;`, and every
    // `ctx.update(…) >> …` became "value >> is not a member of F". A `$` name
    // gets the shape `install_java_module` builds for a class file it has
    // really read: a `ModuleClass` plus its `Module`.
    let module = internal.len() > 1 && internal.ends_with('$');
    // The owner's *own* declarations only. A nested class is always declared
    // by the class its JVM name names, never inherited into it, and searching
    // the parents too made every one of cats' `Foo.Ops` traits resolve to the
    // first one entered: `cats/FlatMap$Ops` asked `FlatMap` for `Ops`, whose
    // linearization reaches `Functor`, which by then had one.
    if module {
        if let Some(m) = st
            .get(owner)
            .members
            .iter()
            .copied()
            .find(|&s| st.get(s).name == simple && st.get(s).kind == SymKind::Module)
        {
            return st.module_class_of(m);
        }
    } else if let Some(id) = st
        .get(owner)
        .members
        .iter()
        .copied()
        .find(|&s| st.get(s).name == simple && st.get(s).kind == SymKind::Class)
    {
        return id;
    }
    if module {
        let flags = Flags::JAVA.with(Flags::MODULE).with(Flags::FINAL);
        let cls = st.alloc(
            format!("{simple}$"),
            owner,
            SymKind::ModuleClass,
            flags,
            internal,
        );
        let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, internal);
        st.get_mut(m).ty = Type::ModuleRef(cls);
        st.get_mut(cls).ty = Type::ModuleRef(cls);
        st.get_mut(cls).parents = vec![Type::AnyRef];
        if owner == st.root {
            st.enter_in_current(&simple, m);
        }
        return cls;
    }
    let id = st.alloc(&simple, owner, SymKind::Class, Flags::JAVA, internal);
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    st.get_mut(id).parents = vec![Type::AnyRef];
    if owner == st.root {
        st.enter_in_current(&simple, id);
    }
    id
}

fn apply_java_class_meta(st: &mut SymbolTable, id: SymbolId, c: &crate::javaclass::JavaClass) {
    // A value class's JVM header says only `extends Object`; `extends
    // AnyVal` exists solely in its ScalaSignature. Lazy pickle adoption can
    // recover that semantic parent before the nested classfile itself is
    // completed. Keep it when installing the classfile metadata, otherwise a
    // later completion silently turns the class back into an ordinary class
    // and calls on its unboxed result become invalid instance calls. fs2's
    // `Stream.PartiallyAppliedFromIterator` is reached in exactly this order
    // while compiling all of Slick.
    let preserve_anyval =
        c.is_scala && st.get(id).parents.iter().any(|p| matches!(p, Type::AnyVal));
    st.binary_read.insert(id.0);
    let mut flags = st.get(id).flags.with(java_class_flags(c));
    if st.get(id).kind == SymKind::ModuleClass {
        flags = flags.with(Flags::MODULE).with(Flags::FINAL);
    }
    st.get_mut(id).flags = flags;
    // `Foo.class` and `Foo$.class` are two JVM views of the same Scala
    // module. Keep whichever spelling established the module-class symbol;
    // replacing `$` with the static-forwarder spelling (or vice versa) would
    // make nested-owner lookup and method codegen order-dependent.
    let keep_module_variant = st.get(id).kind == SymKind::ModuleClass && c.is_scala && {
        let old = st.get(id).jvm_name.as_str();
        let plain = c.internal_name.trim_end_matches('$');
        let impl_name = format!("{plain}$");
        (old == plain && c.internal_name == impl_name)
            || (old == impl_name && c.internal_name == plain)
    };
    if !keep_module_variant {
        st.set_jvm_name(id, c.internal_name.clone());
    }
    // A module class and its term are created together, but a descriptor can
    // make one JVM view (`Foo$`) load before the static-forwarder view
    // (`Foo.class`). Keep their JVM spelling paired when the second view is
    // merged; otherwise codegen can load `Foo.MODULE$` and invoke a method on
    // `Foo$`, which the verifier correctly rejects.
    if st.get(id).kind == SymKind::ModuleClass {
        let owner = st.get(id).owner;
        let module = st
            .get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| st.get(m).kind == SymKind::Module && st.module_class_of(m) == id);
        if let Some(m) = module {
            let jvm = st.get(id).jvm_name.clone();
            st.set_jvm_name(m, jvm);
        }
    }
    if st.get(id).tparams.is_empty() {
        if let Some(sig) = &c.signature {
            if let Some(cs) = crate::javasign::parse_class_sig(sig) {
                install_java_tparams(st, id, &cs.tparams);
            }
        }
    }
    let mut parents = java_parents(st, id, c);
    if preserve_anyval && !parents.iter().any(|p| matches!(p, Type::AnyVal)) {
        parents.push(Type::AnyVal);
    }
    st.get_mut(id).parents = parents;
}

fn java_parents(
    st: &mut SymbolTable,
    class_id: SymbolId,
    c: &crate::javaclass::JavaClass,
) -> Vec<Type> {
    let env = tparam_env(st, class_id);
    if let Some(sig) = &c.signature {
        if let Some(cs) = crate::javasign::parse_class_sig(sig) {
            let mut ps = vec![Type::AnyRef];
            for sup in &cs.supers {
                let ty = jtype_to_type(st, sup, &env);
                if matches!(&ty, Type::Any | Type::AnyRef) {
                    continue;
                }
                if let Type::Class { sym, .. } = &ty {
                    if *sym == st.object_sym {
                        continue;
                    }
                }
                if !ps.iter().any(|p| same_class(p, &ty)) {
                    ps.push(ty);
                }
            }
            return ps;
        }
    }
    let mut ps = vec![Type::AnyRef];
    if let Some(sup) = &c.super_name {
        if sup != "java/lang/Object" {
            let ty = Type::Class {
                sym: find_or_stub_java_class(st, sup),
                args: vec![],
            };
            if !ps.iter().any(|p| same_class(p, &ty)) {
                ps.push(ty);
            }
        }
    }
    for iface in &c.interfaces {
        let ty = Type::Class {
            sym: find_or_stub_java_class(st, iface),
            args: vec![],
        };
        if !ps.iter().any(|p| same_class(p, &ty)) {
            ps.push(ty);
        }
    }
    ps
}

fn same_class(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Class { sym: x, .. }, Type::Class { sym: y, .. }) => x == y,
        _ => false,
    }
}

fn tparam_env(st: &SymbolTable, owner: SymbolId) -> std::collections::HashMap<String, SymbolId> {
    let mut env = std::collections::HashMap::new();
    for id in &st.get(owner).tparams {
        env.insert(st.get(*id).name.clone(), *id);
    }
    env
}

fn java_method_flags(m: &crate::javaclass::JavaMethod, is_scala: bool) -> Flags {
    let mut flags = Flags::JAVA;
    // nsc's `ClassfileParser` maps `ACC_FINAL` to `FINAL` for a Java class
    // file, and `RefChecks` then closes the member: `class R { override def
    // getClass(): Class[R] }` and `class R { def notify(): Unit = () }` are
    // "cannot override final member" (`java.lang.Object`'s `getClass`,
    // `notify`, `notifyAll` and `wait` are all `final`). A Scala class file's
    // `ACC_FINAL` says nothing the pickle does not, and is not read.
    if !is_scala && m.access & 0x0010 != 0 && m.name != "<init>" {
        flags = flags.with(Flags::FINAL);
    }
    if crate::javaclass::is_java_static(m.access) {
        flags = flags.with(Flags::STATIC);
    }
    if crate::javaclass::is_java_abstract(m.access) {
        flags = flags.with(Flags::ABSTRACT);
    }
    if crate::javaclass::is_java_varargs(m.access) {
        flags = flags.with(Flags::VARARGS);
    }
    if crate::javaclass::is_java_protected(m.access) {
        flags = flags.with(Flags::PROTECTED);
    }
    if m.name == "<init>" {
        flags = flags.with(Flags::CONSTRUCTOR);
    }
    // scala.jdk.CollectionConverters implicit classes compile to 1-arg
    // `ListHasAsScala` / `SeqHasAsJava` methods. Mark them so extension
    // search can apply the real jar converters (no fake Buffer/List).
    if m.name.contains("HasAsScala") || m.name.contains("HasAsJava") {
        flags = flags.with(Flags::IMPLICIT);
    }
    flags
}

fn existing_java_method(
    st: &SymbolTable,
    owner: SymbolId,
    m: &crate::javaclass::JavaMethod,
) -> Option<SymbolId> {
    if let Some(id) = st.lookup_member(owner, &m.name).into_iter().find(|&id| {
        let s = st.get(id);
        s.kind == SymKind::Method && s.owner == owner && s.jvm_name == m.desc
    }) {
        return Some(id);
    }
    let arity = desc_param_count(&m.desc);
    let candidates: Vec<SymbolId> = st
        .lookup_member(owner, &m.name)
        .into_iter()
        .filter(|&id| {
            let s = st.get(id);
            s.kind == SymKind::Method
                && s.owner == owner
                && s.jvm_name.is_empty()
                && method_arity(s) == arity
        })
        .collect();
    // Arity alone is not enough when the name is overloaded at one arity.
    // `java.lang.String` declares `indexOf(int)` and `indexOf(String)`, the
    // prelude declares both, and matching by arity stamped each one's
    // descriptor onto whichever symbol `lookup_member` happened to return
    // first. `method_desc_from_sym` prefers `jvm_name` over the symbol's own
    // type, so the two came out *swapped*: `s.indexOf("$mc")` in slick's
    // `ResultConverter.scala` emitted `String.indexOf:(I)I` and failed
    // verification -- silently, because the body lived in a class file
    // nothing loaded until traits became default methods.
    if let Some(id) = candidates
        .iter()
        .copied()
        .find(|&id| method_params_agree(st.get(id), &m.desc))
    {
        return Some(id);
    }
    candidates.first().copied()
}

/// Whether a symbol's declared parameter types can be the ones this JVM
/// descriptor names. Deliberately conservative — `true` unless the two
/// *demonstrably* disagree — so the arity fallback above keeps every match it
/// used to make and only stops making the wrong ones.
fn method_params_agree(s: &crate::symbol::Symbol, desc: &str) -> bool {
    let declared: Vec<Type> = match &s.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        Type::Function { params, .. } => params.clone(),
        _ => return false,
    };
    let named = desc_param_descs(desc);
    declared.len() == named.len()
        && declared
            .iter()
            .zip(&named)
            .all(|(t, d)| param_matches_desc(t, d))
}

/// The parameter descriptor slices of a method descriptor. The scanner owns
/// no input and therefore this does not allocate a `String` per parameter.
fn desc_param_descs(desc: &str) -> Vec<&str> {
    descriptor::method(desc)
        .map(|method| {
            method
                .params
                .into_iter()
                .map(|field| field.source)
                .collect()
        })
        .unwrap_or_default()
}

fn param_matches_desc(ty: &Type, d: &str) -> bool {
    let prim_of = |t: &Type| -> Option<u8> {
        Some(match t {
            Type::Boolean => b'Z',
            Type::Byte => b'B',
            Type::Short => b'S',
            Type::Char => b'C',
            Type::Int => b'I',
            Type::Long => b'J',
            Type::Float => b'F',
            Type::Double => b'D',
            Type::Unit => b'V',
            _ => return None,
        })
    };
    let dc = d.as_bytes().first().copied().unwrap_or(b'?');
    let d_is_prim = matches!(
        dc,
        b'Z' | b'B' | b'S' | b'C' | b'I' | b'J' | b'F' | b'D' | b'V'
    );
    match (prim_of(ty), d_is_prim) {
        (Some(c), true) => c == dc,
        (Some(_), false) | (None, true) => false,
        // Two reference types. `String` against something else is the one
        // distinction the JDK's search overloads turn on; everything else
        // stays unjudged, because a prelude declaration and a class file
        // descriptor disagree in too many harmless ways.
        (None, false) => match ty {
            Type::String => d == "Ljava/lang/String;" || d == "Ljava/lang/Object;",
            Type::Class { .. } | Type::Array(_) => d != "Ljava/lang/String;",
            _ => true,
        },
    }
}

/// A mixin forwarder or bridge in a *generic Scala* class file: a method with
/// no `Signature` attribute, where the erased descriptor is all there is.
///
/// scalac writes a `Signature` for every method whose Scala type mentions a
/// type parameter, so on a class that *has* type parameters an unsigned method
/// is a forwarder or bridge for a declaration that lives, properly typed,
/// somewhere the pickle describes. `scala.collection.immutable.HashMap` carries
/// `public Object filter(Function1)` and `public IterableOps map(Function1)`
/// -- forwarders for the `filter`/`map` its `MapOps` parent declares -- and
/// reading those as `(Any) => Any` / `(Any) => IterableOps` is what made
/// `foundRefs.filter(_._2._2.isEmpty).map { … }` (slick's
/// `compiler/RewriteJoins.scala`) report `value _2 is not a member of Any`.
/// Installing the forwarder hides the real declaration, which ordinary member
/// lookup would otherwise reach through the parent (or have
/// `PickleSupply::complete` supply from an ancestor's pickle on demand).
///
/// Restricted to a class with type parameters: on a monomorphic class an
/// unsigned descriptor is the whole truth, and an `Object` in it is a real
/// `Any`.
fn is_erased_scala_forwarder(
    st: &SymbolTable,
    owner: SymbolId,
    c: &crate::javaclass::JavaClass,
    m: &crate::javaclass::JavaMethod,
) -> bool {
    c.is_scala && m.signature.is_none() && m.name != "<init>" && !st.get(owner).tparams.is_empty()
}

/// Record a class file's default-access member as Scala's `private[<pkg>]`.
///
/// The two notions are the same one: reachable from the member's own package
/// and nowhere else. `Typer::accessible` already enforces `PRIVATE` with a
/// `private_within` qualifier by walking out from the *member's* owner, so the
/// package's simple name is enough to name the boundary unambiguously — two
/// packages called `concurrent` cannot be confused, because the walk starts at
/// the member and stops at the first enclosing package.
///
/// `Flags` is a full `u32`, so this reuses the existing qualifier rather than
/// claiming a 33rd bit.
fn mark_java_package_private(st: &mut SymbolTable, id: SymbolId, owner: SymbolId, access: u16) {
    if !crate::javaclass::is_java_package_private(access) {
        return;
    }
    let mut pkg = owner;
    while !pkg.is_none() && st.get(pkg).kind != SymKind::Package {
        pkg = st.get(pkg).owner;
    }
    if pkg.is_none() {
        return;
    }
    let name = st.get(pkg).name.clone();
    if name.is_empty() {
        return;
    }
    st.get_mut(id).flags = st.get(id).flags.with(Flags::PRIVATE);
    st.get_mut(id).private_within = Some(name);
}

/// The members a *Scala trait* defines itself although its interface declares
/// them `ACC_ABSTRACT`.
///
/// A trait's `def` with a body compiles to a `default` method, so the class
/// file says what the source said. Two shapes do not, and both are ordinary
/// concrete definitions:
///
/// * a **`val` the trait initialises**. The value is assigned by the trait's
///   `$init$` through a synthetic setter named `pkg$Owner$_setter_$x_$eq`, and
///   the getter `x()` is left abstract for the implementing class's field. The
///   setter exists only when the trait has a right-hand side to assign --
///   `val api: API` in `BasicProfile` has none, `val capabilities = …` in the
///   same trait has one -- so it is exactly the evidence wanted.
/// * a **nested `object`**. Its accessor `X(): Owner$X$` is abstract in the
///   interface for the same reason; the module class is what makes it a
///   definition. slick's profile cake has thirteen of them (`SelectPart`,
///   `FromPart`, `DDL`, `Sequence`, …).
///
/// nsc never asks the class file: `DEFERRED` comes from the pickle, where both
/// shapes are concrete. Reading the JVM flag instead asked every
/// `object X extends <jar profile>` to implement fourteen members it inherits
/// (gitbucket's `BlockingPostgresDriver`).
fn scala_trait_defined_members(
    c: &crate::javaclass::JavaClass,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if !c.is_scala {
        return out;
    }
    for m in &c.methods {
        if let Some(rest) = m.name.split("$_setter_$").nth(1) {
            if let Some(n) = rest.strip_suffix("_$eq") {
                out.insert(n.to_string());
            }
        }
        // `()LOwner$Name$;` and the method is called `Name`: the module
        // accessor of an `object Name` nested in this very class.
        let module = format!("(){}${}$;", module_desc_head(&c.internal_name), m.name);
        if m.desc == module {
            out.insert(m.name.clone());
        }
    }
    out
}

/// `L<internal name>` -- the head of the descriptor of a class nested in
/// `internal`, whose own `$` suffix (a module class) is not part of the
/// nesting prefix.
fn module_desc_head(internal: &str) -> String {
    format!("L{}", internal.trim_end_matches('$'))
}

fn fill_java_members(st: &mut SymbolTable, owner: SymbolId, c: &crate::javaclass::JavaClass) {
    let trait_defined = scala_trait_defined_members(c);
    for m in &c.methods {
        if is_erased_scala_forwarder(st, owner, c, m) {
            continue;
        }
        if let Some(id) = existing_java_method(st, owner, m) {
            // The class file's flags, *plus* the two a class file cannot
            // record and only a pickle knows: `implicit` and "this is a val's
            // accessor". Overwriting wholesale made the whole thing
            // order-dependent -- `import profile.api._` supplies
            // `stringColumnType` and friends from the pickle with
            // `Flags::IMPLICIT`, and if anything loaded
            // `JdbcTypesComponent$ImplicitColumnTypes.class` *after* that
            // (naming `Table[…]` as a parent is enough), every one of the 24
            // column types silently stopped being an implicit and slick's
            // whole DSL was "could not find implicit value of type
            // TypedType[String]". Nothing in the bytecode can contradict
            // either flag, so keeping them is not a guess.
            let had = st.get(id).flags;
            let mut f = java_method_flags(m, c.is_scala);
            for pickled in [Flags::IMPLICIT, Flags::ACCESSOR] {
                if had.contains(pickled) {
                    f = f.with(pickled);
                }
            }
            // `Foo.class` and `Foo$.class` are two views of one Scala module.
            // The former's static forwarder methods and the latter's
            // instance methods have identical names/descriptors. Preserve a
            // STATIC bit established by either view while merging the other,
            // so classfile discovery order cannot turn a valid forwarder into
            // an instance call on the wrong JVM receiver.
            if c.is_scala
                && st.get(owner).kind == SymKind::ModuleClass
                && had.contains(Flags::STATIC)
            {
                f = f.with(Flags::STATIC);
            }
            if trait_defined.contains(&m.name) {
                f.set(Flags::ABSTRACT, false);
            }
            st.get_mut(id).flags = f;
            if st.get(id).jvm_name.is_empty() {
                st.set_jvm_name(id, m.desc.clone());
            }
            continue;
        }
        let mut env = tparam_env(st, owner);
        let parsed = m
            .signature
            .as_deref()
            .and_then(crate::javasign::parse_method_sig);
        let (params, ret, mtparams) = if let Some(ms) = parsed {
            let mut tp_ids = Vec::new();
            for p in &ms.tparams {
                let tid = st.alloc(&p.name, owner, SymKind::TypeParam, Flags::EMPTY, "");
                st.get_mut(tid).ty = Type::TypeParam(tid);
                env.insert(p.name.clone(), tid);
                tp_ids.push(tid);
            }
            for (p, tid) in ms.tparams.iter().zip(&tp_ids) {
                let bounds: Vec<Type> = p
                    .bounds
                    .iter()
                    .map(|b| jtype_to_type(st, b, &env))
                    .filter(|t| !matches!(t, Type::Any | Type::AnyRef))
                    .collect();
                st.get_mut(*tid).parents = bounds;
            }
            let params: Vec<Type> = ms
                .params
                .iter()
                .map(|t| jtype_to_type(st, t, &env))
                .collect();
            let ret = java_result_obj(jtype_to_type(st, &ms.ret, &env));
            (params, ret, tp_ids)
        } else {
            let source = if c.is_scala {
                DescriptorSource::ScalaErased
            } else {
                DescriptorSource::Java
            };
            let (p, r) = parse_method_desc_with_source(st, &m.desc, source);
            (p, java_result_obj(r), Vec::new())
        };
        let mut params = params;
        if crate::javaclass::is_java_varargs(m.access) {
            if let Some(last) = params.last_mut() {
                if let Type::Array(elem) = last {
                    *last = Type::Repeated(elem.clone());
                }
            }
        }
        // An `Object[]` parameter keeps its element as `ObjectTpeJava`, which
        // is `=:=` both `Any` and `AnyRef`: without it `Arrays.fill(Object[],
        // Object)` took no `Array[AnyRef]` at all. A varargs `Object...` is a
        // sequence of values and stays `Any*`, so `String.format("%d", 1)`
        // still boxes its argument.
        for p in params.iter_mut() {
            if let Type::Array(elem) = p {
                *p = Type::Array(Box::new(java_array_element((**elem).clone())));
            }
        }
        let names: Vec<String> = (0..params.len()).map(|i| format!("x${i}")).collect();
        let mut flags = java_method_flags(m, c.is_scala);
        if trait_defined.contains(&m.name) {
            flags.set(Flags::ABSTRACT, false);
        }
        let id = add_method_types(st, owner, &m.name, names, params, ret);
        st.get_mut(id).flags = flags;
        mark_java_package_private(st, id, owner, m.access);
        st.set_jvm_name(id, m.desc.clone());
        if !mtparams.is_empty() {
            for tid in &mtparams {
                // Method type parameters had to be allocated before the
                // method symbol existed, so `alloc` temporarily entered them
                // as class members. Move them with their owner: leaving them
                // on the class makes inherited lookup and wildcard imports
                // expose names that exist only inside this method.
                st.get_mut(owner).members.retain(|member| member != tid);
                st.get_mut(*tid).owner = id;
                if !st.get(id).members.contains(tid) {
                    st.get_mut(id).members.push(*tid);
                }
            }
            st.get_mut(id).tparams = mtparams;
        }
    }
    for f in &c.fields {
        if f.name == "MODULE$" {
            continue;
        }
        if st
            .lookup_member(owner, &f.name)
            .iter()
            .any(|&id| st.get(id).kind == SymKind::Term)
        {
            continue;
        }
        // The generic type first, the erased descriptor only as a fallback.
        // A field's `Signature` attribute was read and thrown away, so
        // `scala/collection/concurrent/INodeBase.java`'s `public volatile
        // MainNode<K, V> mainnode` reached the Scala subclass as a raw
        // `MainNode`, and `key`/`value`-shaped inherited fields as `Object`.
        // Methods have taken their signature since they were first loaded;
        // this is the same treatment for fields.
        let env = tparam_env(st, owner);
        let ty = f
            .signature
            .as_deref()
            .and_then(crate::javasign::parse_field_sig)
            .map(|jt| jtype_to_type(st, &jt, &env))
            .unwrap_or_else(|| {
                let source = if c.is_scala {
                    DescriptorSource::ScalaErased
                } else {
                    DescriptorSource::Java
                };
                parse_field_ty_with_source(st, &f.desc, source).0
            });
        let java_object_field = !c.is_scala && ty == Type::Any;
        let ty = if !c.is_scala { java_result_obj(ty) } else { ty };
        let mut flags = Flags::JAVA;
        if f.access & 0x0010 != 0 {
            flags = flags.with(Flags::FINAL);
        }
        if crate::javaclass::is_java_static(f.access) {
            flags = flags.with(Flags::STATIC);
        }
        if crate::javaclass::is_java_protected(f.access) {
            flags = flags.with(Flags::PROTECTED);
        }
        if crate::javaclass::is_java_enum(f.access) {
            flags = flags.with(Flags::ENUM);
        }
        let id = add_term(st, owner, &f.name, ty);
        st.get_mut(id).flags = flags;
        st.get_mut(id).java_object_field = java_object_field;
        mark_java_package_private(st, id, owner, f.access);
        st.set_jvm_name(id, f.desc.clone());
    }
    // A class file's `name$default$n` methods are the only record that its
    // `name` has defaults at all: the parameter-level `DEFAULTPARAM` bit is a
    // pickle's, and a member the pickle path declines (a case class's
    // synthetic companion `apply`, which `Member::is_public_api` filters out)
    // is described by this reader alone. Without the mark, `FieldSerializer()`
    // reached overload selection with no arguments against a four-parameter
    // signature.
    mark_defaults_from_getters(st, owner);
}

fn jtype_to_type(
    st: &mut SymbolTable,
    t: &crate::javasign::JType,
    env: &std::collections::HashMap<String, SymbolId>,
) -> Type {
    use crate::javasign::JType;
    match t {
        JType::Void => Type::Unit,
        JType::Boolean => Type::Boolean,
        // `byte` and `short` used to be read as `Int` because `scala.Byte`
        // and `scala.Short` had no usable JVM representation; they do now, so
        // a Java `byte[]` really is an `Array[Byte]` and `Byte.valueOf(byte)`
        // accepts a `Byte`.
        JType::Byte => Type::Byte,
        JType::Short => Type::Short,
        JType::Int => Type::Int,
        JType::Char => Type::Char,
        JType::Long => Type::Long,
        JType::Float => Type::Float,
        JType::Double => Type::Double,
        JType::Star => Type::Wildcard,
        JType::Extends(t) => Type::BoundedWildcard {
            lo: None,
            hi: Some(Box::new(jtype_to_type(st, t, env))),
        },
        JType::Super(t) => Type::BoundedWildcard {
            lo: Some(Box::new(jtype_to_type(st, t, env))),
            hi: None,
        },
        JType::Var(n) => env
            .get(n)
            .copied()
            .map(Type::TypeParam)
            .unwrap_or(Type::Any),
        JType::Array(e) => Type::Array(Box::new(jtype_to_type(st, e, env))),
        JType::Class { jvm, args } => {
            if jvm == "java/lang/Object" {
                return Type::Any;
            }
            if jvm == "java/lang/String" {
                if args.is_empty() {
                    return Type::String;
                }
            }
            // A generic signature cannot write the bottom types themselves,
            // so scalac stands in with the synthetic runtime placeholder
            // classes: `case object Canceled extends Outcome[Nothing]`'s own
            // class Signature reads `Outcome<Lscala/runtime/Nothing$;>`.
            // The raw descriptor mapping (without generics) already
            // makes this substitution; a generic-signature parent left it as
            // an ordinary class stub named `Nothing$`, so `Outcome[Nothing]`
            // was not recognised as a subtype of `Outcome[Int]` -- the
            // covariance check compared `Nothing$` against `Int` and failed,
            // surfacing as "type mismatch; found: Canceled$ required:
            // Outcome[Int]" for every case object nested in a companion whose
            // trait is parameterized.
            if jvm == "scala/runtime/Nothing$" {
                return Type::Nothing;
            }
            if jvm == "scala/runtime/Null$" {
                return Type::Null;
            }
            let sym = find_or_stub_java_class(st, jvm);
            let as_ = args.iter().map(|a| jtype_to_type(st, a, env)).collect();
            Type::Class { sym, args: as_ }
        }
    }
}

/// nsc's `objToAny` widens a Java *parameter* of type `Object` to `Any`; a
/// **result** of type `Object` stays `AnyRef`, and that is what gives it `eq`,
/// `ne` and `synchronized`. Every `Object` was read as `Any` here, so
/// `cv.unwrapped eq null` (slick's `GlobalConfig`, on typesafe-config's
/// `ConfigValue.unwrapped(): Object`) was "value eq is not a member of Any".
///
/// Narrow the result and array elements, whose JVM component type remains
/// Object. Generic class arguments retain their current representation.
/// Turning `Object` into
/// `AnyRef` *inside* a signature as well is what nsc does, but it also
/// rewrites every `Hashtable<Object, Object>` in sight, and that regressed
/// `IndexedSeq[Any] <: Int => Any` in slick's `HeapBackend`; the wider change
/// bought nothing on slick, so it is not made here.
fn java_result_obj(t: Type) -> Type {
    match t {
        Type::Any => Type::AnyRef,
        Type::Array(elem) => Type::Array(Box::new(java_array_element(*elem))),
        other => other,
    }
}

fn java_array_element(t: Type) -> Type {
    match t {
        Type::Any => Type::JavaObject,
        Type::Array(elem) => Type::Array(Box::new(java_array_element(*elem))),
        other => other,
    }
}

/// One JVM field descriptor as a type. Used by `pickle_supply` to give a
/// `-cp` value class the constructor field its underlying representation is.
pub(crate) fn field_ty_from_desc(st: &mut SymbolTable, desc: &str) -> Type {
    parse_field_ty_with_source(st, desc, DescriptorSource::ScalaErased).0
}

fn method_arity(s: &crate::symbol::Symbol) -> usize {
    let n = s.paramss.iter().flatten().count();
    if n > 0 {
        return n;
    }
    match &s.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().count(),
        Type::Function { params, .. } => params.len(),
        _ => 0,
    }
}

fn desc_param_count(desc: &str) -> usize {
    descriptor::method(desc).map_or(0, |method| method.params.len())
}

#[cfg(test)]
mod descriptor_semantics_tests {
    use super::*;

    #[test]
    fn scala_pickle_mapping_covers_primitives_arrays_objects_and_bottoms() {
        let mut st = SymbolTable::new();
        let (params, ret) = parse_method_desc(
            &mut st,
            "(ZBCSIJFD[[Ljava/lang/String;Lscala/runtime/BoxedUnit;Lpkg/Outer$Inner;)Lscala/runtime/Null$;",
        );

        assert_eq!(
            &params[..8],
            &[
                Type::Boolean,
                Type::Byte,
                Type::Char,
                Type::Short,
                Type::Int,
                Type::Long,
                Type::Float,
                Type::Double,
            ]
        );
        assert_eq!(
            params[8],
            Type::Array(Box::new(Type::Array(Box::new(Type::String))))
        );
        assert_eq!(params[9], Type::Unit);
        let nested = match &params[10] {
            Type::Class { sym, args } => {
                assert!(args.is_empty());
                *sym
            }
            other => panic!("expected nested JVM class, got {other:?}"),
        };
        assert_eq!(st.get(nested).jvm_name, "pkg/Outer$Inner");
        assert_eq!(ret, Type::Null);
        assert_eq!(desc_param_count("(I[[Ljava/lang/String;)V"), 2);
        assert_eq!(
            desc_param_descs("(I[[Ljava/lang/String;)V"),
            vec!["I", "[[Ljava/lang/String;"]
        );
    }

    #[test]
    fn java_and_scala_erased_mapping_keep_boxed_unit_distinct() {
        let mut st = SymbolTable::new();
        let (scala_params, _) = parse_method_desc_with_source(
            &mut st,
            "(Lscala/runtime/BoxedUnit;[Lpkg/Outer$Inner;)V",
            DescriptorSource::ScalaErased,
        );
        assert_eq!(scala_params[0], Type::Unit);

        let (java_params, _) = parse_method_desc_with_source(
            &mut st,
            "(Lscala/runtime/BoxedUnit;[Lpkg/Outer$Inner;)V",
            DescriptorSource::Java,
        );
        assert!(matches!(java_params[0], Type::Class { .. }));
        assert!(!matches!(java_params[0], Type::Unit));
        let inner = match &java_params[1] {
            Type::Array(inner) => match &**inner {
                Type::Class { sym, .. } => *sym,
                other => panic!("expected nested array element class, got {other:?}"),
            },
            other => panic!("expected array, got {other:?}"),
        };
        assert_eq!(st.get(inner).jvm_name, "pkg/Outer$Inner");

        assert_eq!(
            field_ty_from_desc(&mut st, "[Lscala/runtime/BoxedUnit;"),
            Type::Array(Box::new(Type::Unit))
        );
    }
}

#[cfg(test)]
mod method_type_param_tests {
    use super::*;
    use crate::javaclass::{JavaClass, JavaMethod};
    use std::collections::HashSet;

    #[test]
    fn method_type_params_are_owned_by_method_not_class() {
        let mut st = SymbolTable::new();
        let owner = st.alloc("C", st.root, SymKind::Class, Flags::EMPTY, "example/C");
        let class = JavaClass {
            internal_name: "example/C".into(),
            access: 0x0001,
            super_name: Some("java/lang/Object".into()),
            interfaces: Vec::new(),
            methods: vec![JavaMethod {
                name: "id".into(),
                desc: "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".into(),
                access: 0x0001,
                signature: Some("<A:Ljava/lang/Object;B:Ljava/lang/Object;>(TA;TB;)TA;".into()),
            }],
            fields: Vec::new(),
            outer_desc: None,
            signature: None,
            nested_static: false,
            is_scala: false,
            scala_module: false,
            has_module_field: false,
            inner_classes: Vec::new(),
            sole_instance_field: None,
        };

        fill_java_members(&mut st, owner, &class);

        let method = st
            .lookup_member(owner, "id")
            .into_iter()
            .find(|&id| st.get(id).kind == SymKind::Method)
            .expect("generic method");
        let tparams = st.get(method).tparams.clone();
        assert_eq!(tparams.len(), 2);
        assert_eq!(
            tparams.iter().copied().collect::<HashSet<_>>().len(),
            tparams.len(),
            "method type-parameter IDs must be unique"
        );
        assert!(
            tparams
                .iter()
                .all(|&tp| st.get(owner).members.iter().all(|&m| m != tp)),
            "method type parameters must not remain class members"
        );
        assert!(
            tparams.iter().all(|&tp| st.get(tp).owner == method),
            "method type parameters must point back to their method"
        );
        let method_members = &st.get(method).members;
        assert!(
            method_members.iter().copied().collect::<HashSet<_>>().len() == method_members.len(),
            "method members must not contain duplicate IDs"
        );
        for &tp in &tparams {
            assert_eq!(
                method_members.iter().filter(|&&m| m == tp).count(),
                1,
                "each method type parameter must be registered once"
            );
        }
    }
}

#[cfg(test)]
mod module_view_tests {
    use super::*;
    use crate::javaclass::{JavaClass, JavaField, JavaMethod};

    fn object_view(internal_name: &str, carrier: bool) -> JavaClass {
        JavaClass {
            internal_name: internal_name.to_string(),
            access: 0x0001,
            super_name: Some("java/lang/Object".to_string()),
            interfaces: Vec::new(),
            methods: vec![JavaMethod {
                name: "value".to_string(),
                desc: "()I".to_string(),
                access: if carrier { 0x0001 | 0x0008 } else { 0x0001 },
                signature: None,
            }],
            fields: if carrier {
                Vec::new()
            } else {
                vec![JavaField {
                    name: "MODULE$".to_string(),
                    desc: format!("L{internal_name};"),
                    access: 0x0001 | 0x0008 | 0x0010,
                    signature: None,
                }]
            },
            outer_desc: None,
            signature: None,
            nested_static: false,
            is_scala: true,
            scala_module: carrier,
            has_module_field: !carrier,
            inner_classes: Vec::new(),
            sole_instance_field: None,
        }
    }

    #[test]
    fn object_pickle_carrier_never_becomes_the_runtime_module_class() {
        for carrier_first in [true, false] {
            let mut st = SymbolTable::new();
            let owner = ensure_package(&mut st, "example");
            let carrier = object_view("example/O", true);
            let implementation = object_view("example/O$", false);
            let (first, second) = if carrier_first {
                (&carrier, &implementation)
            } else {
                (&implementation, &carrier)
            };

            let cls = install_java_class_in(&mut st, first, owner);
            let again = install_java_class_in(&mut st, second, owner);
            assert_eq!(again, cls, "the two classfiles are one source module");
            assert_eq!(st.get(cls).kind, SymKind::ModuleClass);
            assert_eq!(st.get(cls).jvm_name, "example/O$");

            let module = st
                .get(owner)
                .members
                .iter()
                .copied()
                .find(|&m| st.get(m).kind == SymKind::Module && st.get(m).name == "O")
                .expect("module term");
            assert_eq!(st.module_class_of(module), cls);
            assert_eq!(st.get(module).jvm_name, "example/O$");

            let value = st
                .get(cls)
                .members
                .iter()
                .copied()
                .find(|&m| st.get(m).kind == SymKind::Method && st.get(m).name == "value")
                .expect("implementation method");
            assert!(
                !st.get(value).flags.contains(Flags::STATIC),
                "the carrier's Java forwarder must not replace the module instance method"
            );
        }
    }

    #[test]
    fn scala_classfile_completion_preserves_pickled_anyval_parent() {
        let mut st = SymbolTable::new();
        let owner = ensure_package(&mut st, "example");
        let cls = stub_class_in(
            &mut st,
            "example/Outer$Partial",
            "Partial".to_string(),
            owner,
        );
        // The lazy ScalaSignature reader has the only evidence that this is a
        // value class and has already recovered its representation field.
        st.get_mut(cls).parents.push(Type::AnyVal);
        let field = st.alloc("underlying", cls, SymKind::Term, Flags::EMPTY, "");
        st.get_mut(field).ty = Type::Boolean;
        st.get_mut(cls).ctor_fields.push(field);
        assert!(st.is_value_class(cls));

        // Its JVM class header cannot repeat `extends AnyVal`; it looks like
        // an ordinary final class extending Object.
        let classfile = JavaClass {
            internal_name: "example/Outer$Partial".to_string(),
            access: 0x0001 | 0x0010,
            super_name: Some("java/lang/Object".to_string()),
            interfaces: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            outer_desc: None,
            signature: None,
            nested_static: true,
            is_scala: true,
            scala_module: false,
            has_module_field: false,
            inner_classes: Vec::new(),
            sole_instance_field: None,
        };
        apply_java_class_meta(&mut st, cls, &classfile);

        assert!(
            st.get(cls)
                .parents
                .iter()
                .any(|parent| matches!(parent, Type::AnyVal)),
            "classfile completion must not discard the ScalaSignature-only AnyVal parent"
        );
        assert!(st.is_value_class(cls));
    }
}
