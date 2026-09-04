//! nsc's `SuperAccessors` + `Symbol.makeNotPrivate`: a `private` member that is
//! read from a *different class file* than the one that declares it.
//!
//! `private` in Scala is a *lexical* notion — an anonymous class, a local
//! class, a lambda body and a companion object all sit inside the owner's
//! scope and may name its `private` (even `private[this]`) members. The JVM
//! has no such notion: `ACC_PRIVATE` is per class file, so every one of those
//! reads is an `IllegalAccessError` at run time. nsc's answer, taken at
//! `superaccessors` (before `pickler`, so the pickle carries it too), is to
//! *rename* the member to `<owner full name with '$'>$$<name>` and drop
//! `private`:
//!
//! ```text
//! class Outer { private[this] val secret = 1; def mk = new AnyRef { … secret … } }
//! // scalac 2.13.16:  public final int Outer$$secret;
//! ```
//!
//! Renaming is not decoration. Merely publishing the member under its source
//! name lets a subclass *accidentally override* it:
//!
//! ```scala
//! class P { private[this] def y = 2; def mk = new AnyRef { override def toString = "" + y }.toString }
//! class Q extends P { def y = 9 }        // legal: `private[this] def y` is not inherited
//! ```
//!
//! scalac prints `2` for `new Q().mk()`; a public un-renamed `P.y` makes
//! `invokevirtual P.y` land on `Q.y` and print `9`.
//!
//! Which references count as "a different class file" is not quite nsc's set:
//! we lower every lambda to an anonymous class (nsc uses `invokedynamic` onto
//! a *static* method of the same class), so a `private` member read from a
//! lambda body has to be expanded here where scalac leaves it alone. Expanding
//! more than nsc is harmless — the declaration and every reference are renamed
//! together, and a `private` member cannot be named from another file — while
//! expanding less is an `IllegalAccessError`, so the scan errs towards more.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind};
use std::collections::{HashMap, HashSet};

use crate::lazy_local::children_mut;

/// Rename every `private` member of this unit that is reached from another
/// class, and mark it for the backend to emit without `ACC_PRIVATE`.
pub fn expand_private_names(tree: &mut Tree, st: &mut SymbolTable) {
    widen_private_ctors(tree, st);
    widen_private_case_members(tree, st);
    let mut candidates = HashSet::new();
    collect_private_members(tree, st, &mut candidates);
    if candidates.is_empty() {
        return;
    }
    let mut need = HashSet::new();
    scan(tree, st, SymbolId::NONE, &candidates, &mut need);
    if need.is_empty() {
        return;
    }
    let mut renames: HashMap<SymbolId, (String, String)> = HashMap::new();
    for &id in &need {
        let owner = st.get(id).owner;
        let old = st.get(id).name.clone();
        let new = expanded_name(st, owner, &old);
        st.get_mut(id).name = new.clone();
        st.get_mut(id).access_widened = true;
        renames.insert(id, (old, new));
    }
    rewrite(tree, &renames);
}

/// A `private` constructor called from another class file. A constructor
/// cannot be renamed, so nsc simply drops `private`: `class L private (x: Int)`
/// with a companion that calls `new L(2)` comes out `public L(int)`, while the
/// same class with no such caller stays `private L(int)`. Emitting it
/// `ACC_PRIVATE` regardless made slick's
/// `final class ConstArray private[util] (a: Array[Any], val length: Int) {
/// private def this(a: Array[Any]) = … }` unusable from its own companion:
/// `IllegalAccessError: class slick.util.ConstArray$ tried to access private
/// method ConstArray.<init>`.
fn widen_private_ctors(tree: &mut Tree, st: &mut SymbolTable) {
    let mut cands = HashSet::new();
    collect_private_ctors(tree, st, &mut cands);
    if cands.is_empty() {
        return;
    }
    let mut need = HashSet::new();
    scan(tree, st, SymbolId::NONE, &cands, &mut need);
    for id in need {
        st.get_mut(id).access_widened = true;
    }
}

/// A case class's synthetic `apply` / `copy` made `private` by
/// `-Xsource-features:case-apply-copy-access`, called from another class file.
///
/// `collect_private_members` cannot see these: they are symbols the namer
/// created, with no `DefDef` in the tree to walk. They are reached across a
/// class file boundary all the time and legally — `C(x)` written inside `C`
/// is a call from `C` into `C$`, and a class nested in `C` calling `copy` is
/// a call from `C$Inner` into `C`:
///
/// ```scala
/// case class C private (x: Int) { class Inner { def bump = copy(x = 7) } }
/// ```
///
/// scalac 2.13.16 with the feature emits that `copy` as `public C C$$copy(int)`
/// — nsc's `makeNotPrivate`, which renames as well as widens. Widening alone
/// is enough here and is what `widen_private_ctors` above already does: the
/// rename exists to stop a subclass accidentally overriding the published
/// member, and this compiler has no other member to collide with.
fn widen_private_case_members(tree: &mut Tree, st: &mut SymbolTable) {
    let mut cands = HashSet::new();
    collect_private_case_members(tree, st, &mut cands);
    if cands.is_empty() {
        return;
    }
    let mut need = HashSet::new();
    scan(tree, st, SymbolId::NONE, &cands, &mut need);
    for id in need {
        st.get_mut(id).access_widened = true;
    }
}

fn collect_private_case_members(t: &mut Tree, st: &SymbolTable, out: &mut HashSet<SymbolId>) {
    if let TreeKind::ClassDef { .. } = &t.kind {
        let cls = t.sym;
        if !cls.is_none() && st.get(cls).flags.contains(Flags::CASE) {
            let mut scopes = vec![cls];
            if let Some(m) = st.companion_module(cls) {
                scopes.push(st.module_class_of(m));
            }
            for scope in scopes {
                for &mem in &st.get(scope).members {
                    let s = st.get(mem);
                    if (s.name == "copy" || s.name == "apply")
                        && s.flags.contains(Flags::SYNTHETIC)
                        && s.flags.contains(Flags::PRIVATE)
                        && s.private_within.is_none()
                    {
                        out.insert(mem);
                    }
                }
            }
        }
    }
    for c in children_mut(t) {
        collect_private_case_members(c, st, out);
    }
}

fn collect_private_ctors(t: &mut Tree, st: &SymbolTable, out: &mut HashSet<SymbolId>) {
    if matches!(t.kind, TreeKind::DefDef { .. }) && !t.sym.is_none() {
        let s = st.get(t.sym);
        if s.flags.contains(Flags::PRIVATE)
            && s.private_within.is_none()
            && s.name == "<init>"
            && matches!(st.get(s.owner).kind, SymKind::Class)
        {
            out.insert(t.sym);
        }
    }
    for c in children_mut(t) {
        collect_private_ctors(c, st, out);
    }
}

/// `<owner full name, '$'-separated>$$<name>`, nsc's `nme.expandedName`.
/// A setter keeps its `_$eq` suffix outside the expansion, so `w`'s setter is
/// `Outer$$w_$eq` and not `Outer$$w_$eq`'s literal expansion.
fn expanded_name(st: &SymbolTable, owner: SymbolId, name: &str) -> String {
    let raw = st.jvm_internal(owner);
    // A module class's internal name carries the trailing `$` (`A$B$`); its
    // *symbol* full name does not, and that is what nsc expands against.
    let base = if st.get(owner).kind == SymKind::ModuleClass {
        raw.strip_suffix('$').unwrap_or(&raw)
    } else {
        raw.as_str()
    };
    let prefix = base.replace('/', "$");
    match name.strip_suffix("_$eq") {
        Some(b) => format!("{prefix}$${b}_$eq"),
        None => format!("{prefix}$${name}"),
    }
}

/// The `private` term members declared by this unit. `private[C]` is *not* one
/// of them: it is `PRIVATE` in the tree but compiles to a public member, so
/// renaming it would move a name other files legitimately reference.
fn collect_private_members(t: &mut Tree, st: &SymbolTable, out: &mut HashSet<SymbolId>) {
    if matches!(t.kind, TreeKind::ValDef { .. } | TreeKind::DefDef { .. }) && !t.sym.is_none() {
        let s = st.get(t.sym);
        let owner_kind = st.get(s.owner).kind;
        if s.flags.contains(Flags::PRIVATE)
            && s.private_within.is_none()
            && !s.flags.contains(Flags::PARAM)
            && !s.flags.contains(Flags::CONSTRUCTOR)
            && s.name != "<init>"
            && matches!(s.kind, SymKind::Method | SymKind::Term)
            && matches!(owner_kind, SymKind::Class | SymKind::ModuleClass)
        {
            out.insert(t.sym);
        }
    }
    for c in children_mut(t) {
        collect_private_members(c, st, out);
    }
}

/// Walk with the class the emitted code will *live in*. A `Function` becomes
/// its own class file here (we have no `invokedynamic`), so nothing inside one
/// is in the declaring class any more — `NONE` never matches an owner.
fn scan(
    t: &mut Tree,
    st: &SymbolTable,
    cls: SymbolId,
    candidates: &HashSet<SymbolId>,
    need: &mut HashSet<SymbolId>,
) {
    if !t.sym.is_none() && candidates.contains(&t.sym) && st.get(t.sym).owner != cls {
        need.insert(t.sym);
    }
    let inner = match &t.kind {
        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => code_class_of(st, t.sym, cls),
        TreeKind::Function { .. } => SymbolId::NONE,
        _ => cls,
    };
    for c in children_mut(t) {
        scan(c, st, inner, candidates, need);
    }
}

fn code_class_of(st: &SymbolTable, sym: SymbolId, fallback: SymbolId) -> SymbolId {
    if sym.is_none() {
        return fallback;
    }
    match st.get(sym).kind {
        SymKind::Class | SymKind::ModuleClass => sym,
        SymKind::Module => st.module_class_of(sym),
        _ => fallback,
    }
}

fn rewrite(t: &mut Tree, renames: &HashMap<SymbolId, (String, String)>) {
    if let Some((old, new)) = renames.get(&t.sym) {
        let repl = match &t.kind {
            TreeKind::ValDef { name, .. }
            | TreeKind::DefDef { name, .. }
            | TreeKind::Select { name, .. }
            | TreeKind::Ident { name } => {
                if name == old {
                    Some(new.clone())
                } else if name.strip_suffix("_$eq") == Some(old.as_str()) {
                    Some(format!("{new}_$eq"))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(r) = repl {
            match &mut t.kind {
                TreeKind::ValDef { name, .. }
                | TreeKind::DefDef { name, .. }
                | TreeKind::Select { name, .. }
                | TreeKind::Ident { name } => *name = r,
                _ => {}
            }
        }
    }
    for c in children_mut(t) {
        rewrite(c, renames);
    }
}
