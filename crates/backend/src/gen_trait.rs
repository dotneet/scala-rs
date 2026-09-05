//! `Gen`'s trait and mixin emission, and the members a class owes its
//! parents: `default` bodies with their `m$` statics and `$init$`, mixin
//! fields, `lazy val`s and forwarders, super and outer accessors, the
//! `equals` / `hashCode` / `toString` / `canEqual` of a `case` class, erasure
//! and covariant-return bridges, default getters, member-`object` accessors
//! and the getters of `val` members.

use crate::classfile::{
    encode_method_name, Field, ACC_BRIDGE, ACC_FINAL, ACC_INTERFACE, ACC_PRIVATE, ACC_PUBLIC,
    ACC_STATIC, ACC_SYNTHETIC, ACC_TRANSIENT, ACC_VOLATILE,
};
use crate::gen::*;
use crate::ifacebridge::BridgeKind;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::SymKind;
use std::collections::{HashMap, HashSet};

impl<'a> Gen<'a> {
    /// The concrete half of a trait, emitted **onto the interface itself**,
    /// which is what nsc 2.13 does and what a separately compiled subclass
    /// expects to find (see `docs/notes/bytecode-and-java-interop.md`).
    ///
    /// * a concrete `def m` becomes a `default` method holding the body, plus
    ///   `public static m$($this: T, …)` forwarding to it;
    /// * a genuinely `private def h` becomes `private static h$($this: T, …)`
    ///   holding the body and nothing else — no declaration, no forwarder;
    /// * the `val` initializers and bare statements become
    ///   `public static void $init$(T)`.
    ///
    /// The bodies are byte-for-byte what the old `<Iface>$class` statics held:
    /// slot 0 is the receiver in both shapes, so parameters keep their slots
    /// and the verification types are the same.
    pub(crate) fn emit_trait_bodies(&self, b: &mut ClassBuilder, class_id: SymbolId, iface: &str) {
        let methods = self
            .traits
            .impls
            .get(&class_id)
            .cloned()
            .unwrap_or_default();
        let inits = self
            .traits
            .inits
            .get(&class_id)
            .cloned()
            .unwrap_or_default();
        let lambda_wm = self.lambda_watermark();
        for def in &methods {
            self.emit_trait_impl_method(b, class_id, iface, def);
        }
        // A `lazy val`'s initialiser is a `default` method here too, and the
        // implementing class's `m$lzycompute` is what makes it lazy. Without
        // it a class real scalac compiled died on the first *read* of the
        // `lazy val` with `NoSuchMethodError: 'int L.d$(L)'`.
        for vd in self
            .traits
            .lazy_vals
            .get(&class_id)
            .cloned()
            .unwrap_or_default()
        {
            self.emit_trait_impl_method(b, class_id, iface, &lazy_val_as_def(&vd));
        }
        // Unconditionally, even when there is nothing to run: nsc emits
        // `$init$` on *every* trait interface and every class it compiles
        // calls it for every mixed-in trait, without consulting whether the
        // body would be empty. A trait of ours with no initializers and no
        // `$init$` is a `NoSuchMethodError` in a subclass real scalac built.
        self.emit_trait_init(b, class_id, iface, &inits);
        self.drain_lambdas(b, lambda_wm);
    }

    pub(crate) fn emit_trait_init(
        &self,
        b: &mut ClassBuilder,
        trait_id: SymbolId,
        iface: &str,
        // The `val` initializers interleaved with the trait body's bare
        // statements, in source order.
        inits: &[Tree],
    ) {
        let desc = format!("(L{iface};)V");
        let iface_owned = iface.to_string();
        let st = self.st;
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let inits = inits.to_vec();
        let caps = trait_capture_accessors(self.st, boxed_vars, trait_id);
        let max_locals = 4 + caps.iter().map(|c| c.3.slots()).sum::<u16>();
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            "$init$",
            &desc,
            max_locals,
            |asm| {
                let mut frame = Frame::instance();
                emit_trait_capture_prologue(asm, &mut frame, &iface_owned, &caps);
                let ctx = emit_ctx(
                    st,
                    trait_id,
                    &iface_owned,
                    Type::Unit,
                    extras,
                    lambda_n,
                    lambda_bodies,
                    Some(&hoist_owner),
                    source,
                    library_abi,
                    boxed_vars,
                );
                for vd in &inits {
                    if let TreeKind::ValDef {
                        name, mods, rhs, ..
                    } = &vd.kind
                    {
                        if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                            continue;
                        }
                        asm.aload(0);
                        gen_expr(asm, &mut frame, &ctx, rhs);
                        let ty = val_tree_ty(st, vd);
                        let setter = trait_member_setter_name(
                            st,
                            trait_id,
                            name,
                            mods.flags.contains(Flags::MUTABLE),
                        );
                        // `jvm_desc_val`, not `jvm_desc`: a `Unit` in a value
                        // position erases to `BoxedUnit`, not to `V`.
                        let sdesc = jvm_desc_val(st, &ty);
                        fill_boxed_unit_slot(asm, &sdesc);
                        asm.invokeinterface(&iface_owned, &setter, &format!("({sdesc})V"));
                    } else {
                        // A bare statement of the trait body (SLS 5.1): it runs
                        // from `$init$`, in its source position among the `val`
                        // setter calls.
                        gen_stat(asm, &mut frame, &ctx, vd);
                    }
                }
                asm.vreturn();
            },
        );
    }

    pub(crate) fn emit_trait_impl_method(
        &self,
        b: &mut ClassBuilder,
        trait_id: SymbolId,
        iface: &str,
        def: &Tree,
    ) {
        let (name, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                vparamss,
                rhs,
                ..
            } => (name, vparamss, rhs),
            _ => return,
        };
        if rhs.is_empty() {
            return;
        }
        let inst_desc = def_method_desc(self.st, def);
        let desc = trait_static_desc(iface, &inst_desc);
        let ret = method_ret_ty(def);
        let mut frame = Frame::instance(); // slot 0 = $this
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                frame.alloc_param(p.sym, jvm_sort(&ty), &ty);
            }
        }
        let iface_owned = iface.to_string();
        let st = self.st;
        let max_locals = frame.next_slot;
        let ret_for_body = ret.clone();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let meth = def.sym;
        let caps = trait_capture_accessors(self.st, boxed_vars, trait_id);
        let max_locals = max_locals + caps.iter().map(|c| c.3.slots()).sum::<u16>();
        let private = is_trait_private_def(self.st, def);
        // A genuine `private` has no declaration on the interface and no
        // mixin forwarder, so its body cannot be a `default` method (JVMS
        // §4.6 forbids `ACC_PRIVATE | ACC_ABSTRACT`, and a `private default`
        // would still be unreachable from the classes that mix the trait in).
        // It stays a `private static` taking `$this`, which every caller —
        // all of them textually inside the trait — reaches with a same-class
        // `invokestatic`.
        let (body_acc, body_name, body_desc) = if private {
            (ACC_PRIVATE | ACC_STATIC, trait_static_name(name), desc)
        } else {
            (ACC_PUBLIC, name.clone(), inst_desc.clone())
        };
        b.add_code(body_acc, &body_name, &body_desc, max_locals, |asm| {
            let mut frame = frame;
            emit_trait_capture_prologue(asm, &mut frame, &iface_owned, &caps);
            let mut ctx = emit_ctx(
                st,
                trait_id,
                &iface_owned,
                ret_for_body.clone(),
                extras,
                lambda_n,
                lambda_bodies,
                Some(&hoist_owner),
                source,
                library_abi,
                boxed_vars,
            );
            ctx.method_sym = meth;
            finish_method_body(asm, &mut frame, &ctx, rhs, &ret_for_body);
        });
        if private {
            return;
        }
        // `public static m$($this, …)`: nsc's entry point for the mixin
        // forwarder every implementing class carries and for `super` calls
        // into the trait, forwarding to the `default` method with
        // `invokespecial` on this very interface.
        let iface_c = iface.to_string();
        let name_c = name.clone();
        let inst_c = inst_desc.clone();
        let static_desc = trait_static_desc(iface, &inst_desc);
        let mut locals = 1u16;
        let mut loads = Vec::new();
        for s in desc_param_sorts(desc_params(&inst_desc)) {
            loads.push((locals, s));
            locals += s.slots();
        }
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            &trait_static_name(name),
            &static_desc,
            locals.max(1),
            move |asm| {
                asm.aload(0);
                for (slot, sort) in &loads {
                    load(asm, *slot, *sort);
                }
                asm.invokespecial_interface(&iface_c, &name_c, &inst_c);
                emit_return(asm, &ret);
            },
        );
    }

    /// The `val`s and `var`s a trait read from `-cp` makes every implementing
    /// class carry: `(name, type, is a var)`.
    ///
    /// [`TraitImpls`] is harvested from source trees, so it knows nothing
    /// about a trait that arrived as a class file, and a class of ours mixing
    /// one in used to get no field, no accessor and no `$init$` call at all --
    /// `AbstractMethodError: Receiver class C does not define or inherit an
    /// implementation of the resolved method 'abstract String v()'`.
    ///
    /// The interface's own method table says which members those are, because
    /// nsc declares one mixin setter per concrete `val`
    /// (`p$q$T$_setter_$v_$eq`) and a plain `v_$eq` beside the getter of a
    /// `var`. A *deferred* `var` is indistinguishable from a concrete one
    /// there -- the pickle is what carries `DEFERRED`, and the classfile
    /// scanner does not keep it -- so the `var` half is skipped when anything
    /// else in the linearization already supplies the name (the class itself,
    /// a superclass, or an earlier trait).
    pub(crate) fn binary_trait_vals(&self, trait_id: SymbolId) -> Vec<(String, Type, bool)> {
        let mut out = Vec::new();
        let mut names: HashSet<&str> = HashSet::new();
        for &m in &self.st.get(trait_id).members {
            names.insert(self.st.get(m).name.as_str());
        }
        for &m in &self.st.get(trait_id).members {
            let s = self.st.get(m);
            if s.kind != SymKind::Method {
                continue;
            }
            let Some(base) = s.name.strip_suffix("_=") else {
                continue;
            };
            let Type::Method { paramss, .. } = &s.ty else {
                continue;
            };
            let params: Vec<&Type> = paramss.iter().flatten().collect();
            let [ty] = params[..] else { continue };
            match base.rsplit_once("$_setter_$") {
                // `p$q$T$_setter_$v_$eq(T)`: a concrete `val` named `v`.
                Some((_, field)) => out.push((field.to_string(), ty.clone(), false)),
                // `v_$eq(T)` beside a `v()`: a `var`.
                None if names.contains(base) => out.push((base.to_string(), ty.clone(), true)),
                None => {}
            }
        }
        out
    }

    /// Names a *class* in this linearization already provides, so a binary
    /// trait's `var` is not given a second field that shadows it.
    pub(crate) fn superclass_member_names(&self, class_id: SymbolId) -> HashSet<String> {
        let mut out = HashSet::new();
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            if is_interface_sym(self.st, parent) {
                continue;
            }
            for &m in &self.st.get(parent).members {
                out.insert(self.st.get(m).name.clone());
            }
        }
        out
    }

    /// The extra JVM field flags a mixed-in `val`/`var` keeps from the trait
    /// that declared it. `@volatile` above all: dropping it turns a field the
    /// program declared as volatile into a plain one, which is a memory-model
    /// change no check but `getModifiers` can see (`run/t8087`,
    /// `run/trait_fields_volatile`).
    pub(crate) fn mixin_field_extra_access(v: &Tree) -> u16 {
        let TreeKind::ValDef { mods, .. } = &v.kind else {
            return 0;
        };
        let mut acc = 0;
        if mods.flags.contains(Flags::VOLATILE) {
            acc |= ACC_VOLATILE;
        }
        if mods.flags.contains(Flags::TRANSIENT) {
            acc |= ACC_TRANSIENT;
        }
        acc
    }

    pub(crate) fn mixin_val_fields(
        &self,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
    ) -> Vec<(String, Type, u16)> {
        let mut have = HashSet::new();
        for clause in vparamss {
            for p in clause {
                if let Some(n) = p.name() {
                    have.insert(n.to_string());
                }
            }
        }
        for stt in body {
            if let TreeKind::ValDef { name, .. } = &stt.kind {
                have.insert(name.clone());
            }
        }
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mut from_class: Option<HashSet<String>> = None;
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            let Some(vals) = self.traits.vals.get(&parent) else {
                if !is_interface_sym(self.st, parent) {
                    continue;
                }
                let inherited = from_class
                    .get_or_insert_with(|| self.superclass_member_names(class_id))
                    .clone();
                for (name, ty, mutable) in self.binary_trait_vals(parent) {
                    if mutable && inherited.contains(&name) {
                        continue;
                    }
                    if have.insert(name.clone()) {
                        out.push((name, ty, 0));
                    }
                }
                continue;
            };
            for v in vals {
                let name = v.name().unwrap_or("").to_string();
                if name.is_empty() || !have.insert(name.clone()) {
                    continue;
                }
                out.push((
                    name,
                    val_tree_ty(self.st, v),
                    Self::mixin_field_extra_access(v),
                ));
            }
        }
        out
    }

    /// `lazy val`s inherited from mixed-in traits, in linearization order and
    /// minus anything the class itself (re)defines. nsc's mixin phase copies
    /// the accessor into every implementing class; so do we.
    pub(crate) fn mixin_lazy_vals(&self, class_id: SymbolId, body: &[Tree]) -> Vec<Tree> {
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mut have: HashSet<String> = HashSet::new();
        for stt in body {
            match &stt.kind {
                TreeKind::ValDef { name, .. } | TreeKind::DefDef { name, .. } => {
                    have.insert(name.clone());
                }
                _ => {}
            }
        }
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            if !is_interface_sym(self.st, parent) {
                continue;
            }
            let Some(vals) = self.traits.lazy_vals.get(&parent) else {
                continue;
            };
            for v in vals {
                let name = v.name().unwrap_or("").to_string();
                if name.is_empty() || !have.insert(name.clone()) {
                    continue;
                }
                out.push(v.clone());
            }
        }
        out
    }

    /// Member `object`s inherited from mixed-in traits, in linearization
    /// order. Each needs its own field and accessor on the implementing class,
    /// because the trait can only declare the accessor abstractly.
    pub(crate) fn mixin_member_modules(
        &self,
        class_id: SymbolId,
        own: &[SymbolId],
    ) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mut have: HashSet<String> = own
            .iter()
            .map(|m| module_accessor_name(self.st, *m))
            .collect();
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            if !is_interface_sym(self.st, parent) {
                continue;
            }
            let Some(mods) = self.traits.modules.get(&parent) else {
                continue;
            };
            for m in mods {
                if m.sym.is_none() {
                    continue;
                }
                // A `case class` entry stands for its synthesized companion
                // (see `collect_trait_impls`); a `ModuleDef` is its own.
                let mcls = match &m.kind {
                    TreeKind::ClassDef { .. } => match self.st.companion_module(m.sym) {
                        Some(c) => module_class_id(self.st, c),
                        None => continue,
                    },
                    _ => module_class_id(self.st, m.sym),
                };
                if member_module_outer(self.st, mcls) != Some(parent) {
                    continue;
                }
                if !have.insert(module_accessor_name(self.st, mcls)) {
                    continue;
                }
                out.push(mcls);
            }
        }
        out
    }

    /// `lazy val`s inherited from a trait that arrived as a **class file**.
    ///
    /// On the interface a `lazy val` is indistinguishable from a concrete
    /// trait method -- a `default d()` with a `d$` static beside it -- and the
    /// caching is the implementing class's job. What tells the two apart is
    /// the pickle: a `lazy val`'s accessor is pickled `ACCESSOR`, so
    /// `install_classpath` gives it `SymKind::Term`, while a `def` is a
    /// method. A *non*-lazy trait `val` has no static at all (the class
    /// supplies its value through the mixin setter), so "term plus `d$`" is
    /// exactly the set that needs a field and a `d$lzycompute` here. Without
    /// it the class inherited the interface's `default`, which recomputes the
    /// initialiser on every read.
    pub(crate) fn binary_mixin_lazy_vals(
        &self,
        class_id: SymbolId,
        body: &[Tree],
    ) -> Vec<BinaryLazyVal> {
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mut have: HashSet<String> = HashSet::new();
        for stt in body {
            match &stt.kind {
                TreeKind::ValDef { name, .. } | TreeKind::DefDef { name, .. } => {
                    have.insert(name.clone());
                }
                _ => {}
            }
        }
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            if !is_interface_sym(self.st, parent) || self.traits.impls.contains_key(&parent) {
                continue;
            }
            for m in self.st.get(parent).members.clone() {
                let s = self.st.get(m);
                if s.kind != SymKind::Term || s.flags.contains(Flags::MUTABLE) {
                    continue;
                }
                let name = s.name.clone();
                let ty = s.ty.clone();
                if !self.binary_trait_defines(parent, &name) || !have.insert(name.clone()) {
                    continue;
                }
                out.push(BinaryLazyVal {
                    name,
                    ty,
                    owner: parent,
                });
            }
        }
        out
    }

    /// The class's own `lazy val`s followed by the inherited ones: one list so
    /// they share the `bitmap$N` words without colliding on a bit.
    pub(crate) fn all_lazy_vals(&self, class_id: SymbolId, body: &[Tree]) -> Vec<Tree> {
        let mut out: Vec<Tree> = body
            .iter()
            .filter(|s| match &s.kind {
                TreeKind::ValDef { mods, rhs, .. } => {
                    mods.flags.contains(Flags::LAZY) && !rhs.is_empty()
                }
                _ => false,
            })
            .cloned()
            .collect();
        out.extend(self.mixin_lazy_vals(class_id, body));
        out
    }

    /// The `bitmap$N` fields a class needs for `lazies`.
    ///
    /// One `int` holds 32 initialisation bits. A single word used to be
    /// assumed, and the 33rd `lazy val` in a class then got `1 << 32`, which
    /// the JVM (and Rust's shift) reduces to `1 << 0`: the value shared bit 0
    /// with the first `lazy val`, so forcing that one made every later
    /// accessor report itself initialised and return the field's default.
    /// `run/t3038c` in scala/scala is exactly that program -- 70 `lazy val`s,
    /// of which we printed the first 32 and then zeros.
    pub(crate) fn lazy_bitmap_fields(&self, lazies: &[Tree], binary: usize) -> Vec<Field> {
        let n = binary
            + lazies
                .iter()
                .filter(|stt| match &stt.kind {
                    TreeKind::ValDef { mods, rhs, .. } => {
                        mods.flags.contains(Flags::LAZY) && !rhs.is_empty()
                    }
                    _ => false,
                })
                .count();
        let words = n.div_ceil(32).max(1);
        (0..words)
            .map(|w| Field {
                access: ACC_PRIVATE,
                name: format!("bitmap${w}"),
                desc: "I".into(),
            })
            .collect()
    }

    pub(crate) fn emit_trait_val_accessors(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
    ) {
        if class_id.is_none() {
            return;
        }
        let mut skip = HashSet::new();
        for stt in body {
            if let TreeKind::DefDef { name, .. } = &stt.kind {
                skip.insert(name.clone());
            }
        }
        for m in &b.methods {
            skip.insert(m.name.clone());
        }
        // (name, type, owning trait, is a `var`, first in linearization order,
        //  the trait declared it `final`)
        let mut needed: Vec<(String, Type, SymbolId, bool, bool, bool)> = Vec::new();
        let mut seen = HashSet::new();
        let mut from_class: Option<HashSet<String>> = None;
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            let Some(vals) = self.traits.vals.get(&parent) else {
                // A trait read from `-cp` has no source tree to harvest; its
                // interface says what the class owes. See `binary_trait_vals`.
                if !is_interface_sym(self.st, parent) {
                    continue;
                }
                let inherited = from_class
                    .get_or_insert_with(|| self.superclass_member_names(class_id))
                    .clone();
                for (name, ty, mutable) in self.binary_trait_vals(parent) {
                    if mutable && inherited.contains(&name) {
                        continue;
                    }
                    let first = seen.insert(name.clone());
                    needed.push((name, ty, parent, mutable, first, false));
                }
                continue;
            };
            for v in vals {
                let name = v.name().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                // Deliberately *not* `continue` on a repeat: two traits in one
                // linearization may both define `val v`, the later one an
                // `override` with a narrower type (slick's
                // `SqlTableComponent.columnOptions` over
                // `RelationalTableComponent.columnOptions`). Each declares its
                // own `_setter_` on its own interface, so the class owes an
                // implementation of *both* -- skipping the base one left
                // `H2Profile$` failing `RelationalTableComponent.$init$` with
                // an `AbstractMethodError`.
                let first = seen.insert(name.clone());
                let (mutable, is_final) = match &v.kind {
                    TreeKind::ValDef { mods, .. } => (
                        mods.flags.contains(Flags::MUTABLE),
                        mods.flags.contains(Flags::FINAL),
                    ),
                    _ => (false, false),
                };
                needed.push((
                    name,
                    val_tree_ty(self.st, v),
                    parent,
                    mutable,
                    first,
                    is_final,
                ));
            }
        }
        let class_name = b.this_name.clone();
        for (name, ty, owner, mutable, first, is_final) in needed {
            // nsc's mixin phase carries the trait `val`'s `final` onto the
            // accessors it copies into the class: `trait T { final val v = 1 }`
            // gives `public final int v()` and a final `T$_setter_$v_$eq`
            // (`run/trait_fields_bytecode`, `run/trait_fields_final`). A plain
            // `val` gets neither.
            let fin = if is_final { ACC_FINAL } else { 0 };
            let fdesc = jvm_desc_val(self.st, &ty);
            let gdesc = format!("(){}", jvm_desc(self.st, &ty));
            let sdesc = format!("({fdesc})V");
            let sort = jvm_slot_sort(&ty);
            let setter = trait_member_setter_name(self.st, owner, &name, mutable);
            // `override val v = …`: the value is provided elsewhere -- by the
            // class itself, or by a trait nearer the front of the
            // linearization -- but this trait's mixin setter is still abstract
            // on its interface. nsc implements it as a no-op so `<Iface>.$init$`
            // does not clobber the override, and adds a bridge getter when the
            // override's erased result type is narrower.
            if !first || skip.contains(&name) {
                if !mutable && !skip.contains(&setter) {
                    b.add_code(ACC_PUBLIC, &setter, &sdesc, 1 + sort.slots(), |asm| {
                        asm.vreturn();
                    });
                    skip.insert(setter);
                }
                let narrow = b
                    .methods
                    .iter()
                    .find(|m| m.name == name && m.desc.starts_with("()"))
                    .map(|m| m.desc.clone());
                if let Some(nd) = narrow {
                    if nd != gdesc && nd.starts_with("()L") && gdesc.starts_with("()L") {
                        let cn = class_name.clone();
                        let n2 = name.clone();
                        b.add_code(ACC_PUBLIC | ACC_BRIDGE, &name, &gdesc, 1, move |asm| {
                            asm.aload(0);
                            asm.invokevirtual(&cn, &n2, &nd);
                            asm.areturn();
                        });
                    }
                }
                continue;
            }
            let fname = name.clone();
            let class_c = class_name.clone();
            let fdesc_c = fdesc.clone();
            b.add_code(ACC_PUBLIC | fin, &name, &gdesc, 1, |asm| {
                asm.aload(0);
                emit_getfield(asm, &class_c, &fname, &fdesc_c);
                emit_return(asm, &ty);
            });
            let fname = name.clone();
            let class_c = class_name.clone();
            let fdesc_c = fdesc.clone();
            b.add_code(ACC_PUBLIC | fin, &setter, &sdesc, 1 + sort.slots(), |asm| {
                asm.aload(0);
                load(asm, 1, sort);
                asm.putfield(&class_c, &fname, &fdesc_c);
                asm.vreturn();
            });
            skip.insert(name);
            skip.insert(setter);
        }
    }

    /// Whether a trait that arrived as a *class file* defines `method`.
    ///
    /// [`TraitImpls`] is harvested from source trees, so a trait read from
    /// `-cp` has no entry there at all and every tree-driven decision below
    /// silently treated its concrete members as declarations. The interface
    /// says so on its own: nsc puts a `public static m$(Iface, …)` beside
    /// every concrete trait method (that static is what every mixin forwarder
    /// and every `super` call goes through), and nothing else on an interface
    /// carries that name. So the static *is* the mark of a definition, and it
    /// is in the symbol table because the class file scanner installs an
    /// interface's methods whole.
    pub(crate) fn binary_trait_defines(&self, trait_id: SymbolId, method: &str) -> bool {
        let want = trait_static_name(method);
        self.st.get(trait_id).members.iter().any(|&m| {
            let s = self.st.get(m);
            s.kind == SymKind::Method && s.name == want
        })
    }

    /// The `p$q$T$$super$m` accessors a trait read from `-cp` declares, as
    /// `(accessor symbol, the source name of `m`)`.
    ///
    /// A stackable `abstract override` layer reaches the next one through this
    /// accessor, and the *class* owes the implementation. Without it a binary
    /// stackable trait of nsc's mixed in by us was silent: the JVM resolves a
    /// class method ahead of an interface `default`, so `new Stacked().label`
    /// ran the base implementation and printed the wrong answer with no
    /// exception anywhere.
    pub(crate) fn binary_trait_super_accessors(
        &self,
        trait_id: SymbolId,
    ) -> Vec<(SymbolId, String)> {
        if self.traits.impls.contains_key(&trait_id) {
            return Vec::new();
        }
        let prefix = format!(
            "{}$$super$",
            class_internal(self.st, trait_id).replace('/', "$")
        );
        let members = self.st.get(trait_id).members.clone();
        let mut out = Vec::new();
        for acc in members.iter().copied() {
            let s = self.st.get(acc);
            if s.kind != SymKind::Method {
                continue;
            }
            let Some(enc) = s.name.strip_prefix(&prefix) else {
                continue;
            };
            let enc = enc.to_string();
            // The accessor's name holds the *encoded* method name; the
            // interface's own member list is where the source name is.
            let Some(name) = members.iter().copied().find_map(|m| {
                let t = self.st.get(m);
                (t.kind == SymKind::Method && encode_method_name(&t.name) == enc)
                    .then(|| t.name.clone())
            }) else {
                continue;
            };
            out.push((acc, name));
        }
        out
    }

    pub(crate) fn emit_super_accessors(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let lin = linearize(self.st, class_id);
        for (idx, parent) in lin.iter().enumerate() {
            if idx == 0 || !is_interface_sym(self.st, *parent) {
                continue;
            }
            // `(source name, accessor name, instance descriptor, parameters,
            // result)`. A trait of this run's own sources contributes one
            // entry per member whose body writes `super.m`; a trait read from
            // `-cp` contributes one per accessor its *interface* declares,
            // which is the same set as far as the class is concerned.
            let mut owed: Vec<(String, String, String, Vec<Type>, Type)> = Vec::new();
            match self.traits.impls.get(parent) {
                Some(methods) => {
                    for m in methods {
                        if !needs_super_accessor(m) {
                            continue;
                        }
                        let name = m.name().unwrap_or("").to_string();
                        if name.is_empty() {
                            continue;
                        }
                        owed.push((
                            name.clone(),
                            super_accessor_name(self.st, *parent, &name),
                            def_method_desc(self.st, m),
                            def_param_types(self.st, m),
                            method_ret_ty(m),
                        ));
                    }
                }
                None => {
                    for (acc, name) in self.binary_trait_super_accessors(*parent) {
                        let aname = self.st.get(acc).name.clone();
                        if b.methods.iter().any(|m| m.name == aname) {
                            continue;
                        }
                        owed.push((
                            name,
                            aname,
                            method_desc_from_sym(self.st, acc),
                            method_params_from_sym(self.st, acc),
                            method_ret_from_sym(self.st, acc),
                        ));
                    }
                }
            }
            for (name, acc, inst_desc, pts, ret) in owed {
                let mut locals = 1u16;
                let mut loads = Vec::new();
                for p in &pts {
                    let sort = jvm_sort(p);
                    loads.push((locals, sort));
                    locals += sort.slots();
                }
                let target = self.next_lin_impl(&lin, idx, &name);
                let acc_c = acc.clone();
                let inst_c = inst_desc.clone();
                // The accessor's own signature is the *overriding* method's --
                // that is what the trait's code calls -- but the method it
                // forwards to keeps the signature it was compiled with, and a
                // refined abstract type member (`type RowsPerStatement =
                // One.type` over `>: One.type <: RowsPerStatement`) makes the
                // two differ. Call the target at *its* descriptor.
                let call_desc = self
                    .super_target_desc(target, &name, pts.len())
                    .unwrap_or_else(|| inst_c.clone());
                let call_c = call_desc.clone();
                b.add_code(ACC_PUBLIC, &acc_c, &inst_c, locals.max(1), |asm| {
                    asm.aload(0);
                    for (slot, sort) in &loads {
                        load(asm, *slot, *sort);
                    }
                    match target {
                        Some((next, true)) => {
                            let iface = class_internal(self.st, next);
                            let static_desc = trait_static_desc(&iface, &call_c);
                            asm.invokestatic_interface(
                                &iface,
                                &trait_static_name(&name),
                                &static_desc,
                            );
                        }
                        Some((next, false)) => {
                            let owner = class_internal(self.st, next);
                            asm.invokespecial(&owner, &name, &call_c);
                        }
                        None => {
                            throw_runtime(asm, &format!("no super implementation for {name}"));
                            if !is_unit_like(&ret) {
                                push_default(asm, &ret);
                            }
                        }
                    }
                    // A narrower result than the target declares needs the
                    // cast the accessor's own descriptor promises.
                    if target.is_some() {
                        if let Some(c) = narrowing_return_cast(&call_c, &inst_c) {
                            asm.checkcast(&c);
                        }
                    }
                    emit_return(asm, &ret);
                });
            }
        }
    }

    /// The descriptor the `super` target was compiled with, when it can be
    /// found. `arity` disambiguates overloads.
    pub(crate) fn super_target_desc(
        &self,
        target: Option<(SymbolId, bool)>,
        method: &str,
        arity: usize,
    ) -> Option<String> {
        match target? {
            (next, true) => match self.traits.impls.get(&next) {
                Some(ms) => ms
                    .iter()
                    .find(|m| {
                        m.name() == Some(method) && def_param_types(self.st, m).len() == arity
                    })
                    .map(|m| def_method_desc(self.st, m)),
                // A trait read from `-cp`: its interface carries the
                // descriptor the trait was compiled with.
                None => self
                    .st
                    .get(next)
                    .members
                    .iter()
                    .copied()
                    .find(|&mid| {
                        let mem = self.st.get(mid);
                        mem.name == method
                            && mem.kind == SymKind::Method
                            && param_count(self.st, mid) == arity
                    })
                    .map(|mid| method_desc_from_sym(self.st, mid)),
            },
            (next, false) => self
                .st
                .get(next)
                .members
                .iter()
                .copied()
                .find(|&mid| {
                    let mem = self.st.get(mid);
                    mem.name == method
                        && mem.kind == SymKind::Method
                        && !mem.flags.contains(Flags::ABSTRACT)
                        && param_count(self.st, mid) == arity
                })
                .map(|mid| method_desc_from_sym(self.st, mid)),
        }
    }

    pub(crate) fn next_lin_impl(
        &self,
        lin: &[SymbolId],
        after_idx: usize,
        method: &str,
    ) -> Option<(SymbolId, bool)> {
        for &s in lin.iter().skip(after_idx + 1) {
            if let Some(ms) = self.traits.impls.get(&s) {
                // A trait-private method never dispatches through `super`:
                // it isn't part of the interface's signature, so it can't be
                // the target of another trait's or class's `super.m()`.
                if ms
                    .iter()
                    .any(|m| m.name() == Some(method) && !is_trait_private_def(self.st, m))
                {
                    return Some((s, true));
                }
            } else if is_interface_sym(self.st, s) && self.binary_trait_defines(s, method) {
                // A trait read from `-cp` sitting between two stackable layers
                // of ours: without this the `super` chain skipped it and went
                // straight to the superclass, dropping its layer silently.
                return Some((s, true));
            }
            if !is_interface_sym(self.st, s) {
                let has = self.st.get(s).members.iter().any(|&mid| {
                    let mem = self.st.get(mid);
                    mem.name == method
                        && mem.kind == SymKind::Method
                        && !mem.flags.contains(Flags::ABSTRACT)
                });
                if has {
                    return Some((s, false));
                }
            }
        }
        None
    }

    /// The descriptor of an implementation on this class that *overrides*
    /// `def` at a strictly narrower erased parameter list, if there is exactly
    /// one.
    ///
    /// Two tests, and both are needed. `bridge_overrides` says the two really
    /// are one member -- a parameter that erases to `Object`, or that
    /// `erased_abstract_params` records as abstract before erasure, may be
    /// narrowed by an override; two unrelated `f(Any)` / `f(String)` overloads
    /// may not. `desc_narrows` then fixes the *direction*, which
    /// `bridge_overrides` alone does not: without it the narrow method would
    /// find the wide one just as readily and both would bridge to each other.
    pub(crate) fn narrower_override(
        &self,
        name: &str,
        def: &Tree,
        impls: &[(String, String, Vec<Type>, SymbolId)],
    ) -> Option<String> {
        let enc = encode_method_name(name);
        let wide_desc = def_method_desc(self.st, def);
        let wide_params = desc_params(&wide_desc).to_string();
        let wide_strs = desc_param_strs(&wide_desc);
        let declared = def_param_types(self.st, def);
        let abstract_mask = self
            .st
            .erased_abstract_params
            .get(&def.sym)
            .copied()
            .unwrap_or(0);
        let mut hits = impls.iter().filter(|(n, d, cps, sym)| {
            *n == enc
                && *sym != def.sym
                && desc_params(d) != wide_params
                && bridge_overrides(self.st, &declared, cps, abstract_mask)
                && {
                    let cs = desc_param_strs(d);
                    cs.len() == wide_strs.len()
                        && cs
                            .iter()
                            .zip(&wide_strs)
                            .all(|(c, p)| self.desc_narrows(p, c))
                }
        });
        let first = hits.next().map(|(_, d, _, _)| d.clone())?;
        hits.next().is_none().then_some(first)
    }

    /// Whether a class on the superclass chain already declares a concrete
    /// method that *is* the trait member `def` -- the same member, possibly at
    /// a narrower erased descriptor, which `bridge_overrides` is the test for.
    /// Used only for traits that sit past the superclass in the linearization
    /// (see `emit_mixin_forwarders`).
    pub(crate) fn superclass_implements(
        &self,
        super_impls: &[(String, Vec<Type>, SymbolId)],
        def: &Tree,
    ) -> bool {
        let enc = encode_method_name(def.name().unwrap_or(""));
        let declared = def_param_types(self.st, def);
        let abstract_mask = self
            .st
            .erased_abstract_params
            .get(&def.sym)
            .copied()
            .unwrap_or(0);
        super_impls.iter().any(|(n, cps, sym)| {
            *n == enc && *sym != def.sym && bridge_overrides(self.st, &declared, cps, abstract_mask)
        })
    }

    pub(crate) fn emit_mixin_forwarders(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
    ) {
        if class_id.is_none() {
            return;
        }
        // Keyed by name *and erased parameter list*: a trait may declare
        // several overloads of one name and every one of them needs its own
        // forwarder. slick's `JdbcBackend` has three `makeDatabase`s, and
        // `JdbcBackend$` used to get a forwarder for whichever came first --
        // `JdbcDatabaseConfig.makeDatabase` then threw `AbstractMethodError`.
        // The return type is deliberately *not* part of the key: a class that
        // narrows an inherited member covariantly still overrides it, and the
        // bridge for the wide descriptor is `emit_erasure_bridges`' business,
        // not a forwarder to the trait's own body.
        let mut defined: HashSet<(String, String)> = HashSet::new();
        for stt in body {
            if let TreeKind::DefDef { name, .. } = &stt.kind {
                defined.insert((
                    encode_method_name(name),
                    desc_params(&def_method_desc(self.st, stt)).to_string(),
                ));
            }
        }
        for m in &b.methods {
            defined.insert((m.name.clone(), desc_params(&m.desc).to_string()));
        }
        let lin = linearize(self.st, class_id);
        // Where the superclass sits in the linearization. Every trait *after*
        // it is an ancestor of that class rather than a mixin of this one, so
        // the superclass has already settled which body wins -- and if it
        // narrowed the member, its own erasure bridge settles the wide
        // descriptor too. Emitting a forwarder here would override both.
        //
        // slick's `abstract class JdbcDatabaseDef` overrides
        // `BasicDatabaseDef.setupTransaction(session: Session, …)` at the
        // narrowed `Session = JdbcSessionDef`, and `new JdbcDatabaseDef(…){}`
        // -- the anonymous class every `Database` really is -- got a forwarder
        // for the wide descriptor straight to `BasicDatabaseDef`'s own body, whose
        // body is `None`. Every `.transactionally` therefore ran with
        // autocommit still on and rolled nothing back.
        let super_idx = lin
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, p)| !is_interface_sym(self.st, **p))
            .map(|(i, _)| i);
        let super_impls: Vec<(String, Vec<Type>, SymbolId)> = match super_idx {
            None => Vec::new(),
            Some(_) => lin
                .iter()
                .skip(1)
                .filter(|p| !is_interface_sym(self.st, **p))
                .flat_map(|p| self.st.get(*p).members.iter().copied())
                .filter_map(|mid| {
                    let s = self.st.get(mid);
                    if s.kind != SymKind::Method
                        || s.name == "<init>"
                        || s.flags.contains(Flags::ABSTRACT)
                    {
                        return None;
                    }
                    Some((
                        encode_method_name(&s.name),
                        method_params_from_sym(self.st, mid),
                        mid,
                    ))
                })
                .collect(),
        };
        let mut chosen: Vec<(String, String, Tree)> = Vec::new();
        let mut seen = HashSet::new();
        for (pi, parent) in lin.iter().enumerate().skip(1) {
            let Some(methods) = self.traits.impls.get(parent) else {
                continue;
            };
            if !is_interface_sym(self.st, *parent) {
                continue;
            }
            let past_superclass = super_idx.is_some_and(|si| pi > si);
            let iface = class_internal(self.st, *parent);
            for m in methods {
                let name = m.name().unwrap_or("").to_string();
                // A trait-private method has no interface signature to
                // implement, so no mixing class ever needs a forwarder for
                // it -- and its name must not shadow a same-named *public*
                // method a farther trait in the linearization does need one
                // for.
                if name.is_empty() || is_trait_private_def(self.st, m) {
                    continue;
                }
                let key = (
                    encode_method_name(&name),
                    desc_params(&def_method_desc(self.st, m)).to_string(),
                );
                if !seen.insert(key) {
                    continue;
                }
                if past_superclass && self.superclass_implements(&super_impls, m) {
                    continue;
                }
                chosen.push((name, iface.clone(), m.clone()));
            }
        }
        // Everything this class will implement: its own body, and the
        // forwarders about to be emitted. Two clauses of the linearization can
        // spell *one* member at two erased descriptors, and then the wider one
        // must not run its own trait's body.
        let impls: Vec<(String, String, Vec<Type>, SymbolId)> = body
            .iter()
            .filter_map(|stt| match &stt.kind {
                TreeKind::DefDef { name, .. } => Some((
                    encode_method_name(name),
                    def_method_desc(self.st, stt),
                    def_param_types(self.st, stt),
                    stt.sym,
                )),
                _ => None,
            })
            .chain(chosen.iter().map(|(n, _, d)| {
                (
                    encode_method_name(n),
                    def_method_desc(self.st, d),
                    def_param_types(self.st, d),
                    d.sym,
                )
            }))
            .collect();
        for (name, iface, def) in &chosen {
            let (name, iface, def) = (name.clone(), iface.clone(), def.clone());
            let inst_desc = def_method_desc(self.st, &def);
            if defined.contains(&(
                encode_method_name(&name),
                desc_params(&inst_desc).to_string(),
            )) {
                continue;
            }
            // `SynchronousDatabaseAction.openStream(context: C)` with
            // `C <: BasicBackend#BasicActionContext` is *overridden* by
            // `StreamingInvokerAction.openStream(ctx: JdbcBackend#JdbcActionContext)`,
            // and the two erase to different descriptors. Forwarding both to
            // their own trait bodies leaves the wide one -- which is what a
            // call through the base interface resolves to -- running the base
            // implementation, and slick's base implementation is
            // `throw new SlickException("Streaming is not supported")`.
            // nsc emits the wide descriptor as a bridge to the narrow one.
            if let Some(target) = self.narrower_override(&name, &def, &impls) {
                let ret = method_ret_ty(&def);
                let pstrs = desc_param_strs(&inst_desc);
                let cstrs = desc_param_strs(&target);
                let mut loads: Vec<(u16, JvmSort, Option<String>)> = Vec::new();
                let mut locals = 1u16;
                for (i, s) in desc_param_sorts(desc_params(&inst_desc))
                    .into_iter()
                    .enumerate()
                {
                    let cast = match (pstrs.get(i), cstrs.get(i)) {
                        (Some(p), Some(c)) if p != c && c.starts_with('L') => {
                            Some(c[1..c.len() - 1].to_string())
                        }
                        (Some(p), Some(c)) if p != c && c.starts_with('[') => Some(c.clone()),
                        _ => None,
                    };
                    loads.push((locals, s, cast));
                    locals += s.slots();
                }
                let cn = b.this_name.clone();
                let enc = encode_method_name(&name);
                let enc_c = enc.clone();
                let tdesc = target.clone();
                b.add_code(
                    ACC_PUBLIC | ACC_BRIDGE | ACC_SYNTHETIC,
                    &enc_c,
                    &inst_desc,
                    locals.max(1),
                    move |asm| {
                        asm.aload(0);
                        for (slot, sort, cast) in &loads {
                            load(asm, *slot, *sort);
                            if let Some(c) = cast {
                                asm.checkcast(c);
                            }
                        }
                        asm.invokevirtual(&cn, &enc, &tdesc);
                        emit_return(asm, &ret);
                    },
                );
                continue;
            }
            let static_desc = trait_static_desc(&iface, &inst_desc);
            let ret = method_ret_ty(&def);
            let pts = def_param_types(self.st, &def);
            let mut locals = 1u16;
            let mut loads = Vec::new();
            for p in &pts {
                // A forwarder passes its arguments straight on, so it moves
                // what the JVM actually handed it: `Unit` arrives as a
                // `BoxedUnit` reference.
                let sort = jvm_slot_sort(p);
                loads.push((locals, sort));
                locals += sort.slots();
            }
            let name_c = name.clone();
            let inst_c = inst_desc.clone();
            let static_c = static_desc.clone();
            // nsc's shape: `invokestatic <Iface>.m$`, an `InterfaceMethodref`.
            // `invokespecial` on the `default` method would need the
            // interface to be a *direct* superinterface of this class, which
            // a trait several steps up the linearization is not.
            let iface_c = iface.clone();
            let static_name = trait_static_name(&name);
            b.add_code(ACC_PUBLIC, &name_c, &inst_c, locals.max(1), |asm| {
                asm.aload(0);
                for (slot, sort) in &loads {
                    load(asm, *slot, *sort);
                }
                asm.invokestatic_interface(&iface_c, &static_name, &static_c);
                emit_return(asm, &ret);
            });
        }
        self.emit_binary_mixin_forwarders(b, &lin, super_idx, &super_impls);
        self.emit_trait_capture_accessors(b, class_id, &lin);
        if !self.library_abi {
            let by_name: HashSet<String> = defined.iter().map(|(n, _)| n.clone()).collect();
            self.emit_ordered_forwarders(b, class_id, &by_name);
        }
    }

    /// Mixin forwarders for a trait that arrived as a **class file**.
    ///
    /// nsc puts a forwarder in the class for every concrete member of every
    /// mixed-in trait. We only owe one where the JVM would otherwise disagree
    /// with the linearization, and there is exactly one such shape: a class
    /// method always beats an interface `default`, so a trait member the
    /// *superclass* also defines concretely never runs. That is what made a
    /// binary stackable trait silent -- `class Stacked extends Plain with
    /// Loud` resolved `label()` to `Plain.label()` and never reached
    /// `Loud`'s `default`, with no exception and no diagnostic. Everywhere
    /// else the JVM's own most-specific-interface rule already picks what SLS
    /// 5.1.2 picks, and emitting a forwarder would only restate it -- for
    /// every concrete member of every scala-library trait we mix in.
    ///
    /// Traits *after* the superclass in the linearization are its ancestors,
    /// not this class's mixins: it has already settled which body wins.
    pub(crate) fn emit_binary_mixin_forwarders(
        &self,
        b: &mut ClassBuilder,
        lin: &[SymbolId],
        super_idx: Option<usize>,
        super_impls: &[(String, Vec<Type>, SymbolId)],
    ) {
        let Some(super_idx) = super_idx else {
            return;
        };
        let mut defined: HashSet<(String, String)> = b
            .methods
            .iter()
            .map(|m| (m.name.clone(), desc_params(&m.desc).to_string()))
            .collect();
        for (pi, parent) in lin.iter().enumerate().skip(1) {
            if pi > super_idx
                || !is_interface_sym(self.st, *parent)
                || self.traits.impls.contains_key(parent)
            {
                continue;
            }
            let iface = class_internal(self.st, *parent);
            for mid in self.st.get(*parent).members.clone() {
                let s = self.st.get(mid);
                if s.kind != SymKind::Method || s.name == "<init>" {
                    continue;
                }
                let name = s.name.clone();
                if !self.binary_trait_defines(*parent, &name)
                    || !self.superclass_implements_sym(super_impls, mid)
                {
                    continue;
                }
                let inst_desc = method_desc_from_sym(self.st, mid);
                let key = (
                    encode_method_name(&name),
                    desc_params(&inst_desc).to_string(),
                );
                if !defined.insert(key) {
                    continue;
                }
                let ret = method_ret_from_sym(self.st, mid);
                let static_desc = trait_static_desc(&iface, &inst_desc);
                let mut locals = 1u16;
                let mut loads = Vec::new();
                for sort in desc_param_sorts(desc_params(&inst_desc)) {
                    loads.push((locals, sort));
                    locals += sort.slots();
                }
                let iface_c = iface.clone();
                let static_name = trait_static_name(&name);
                b.add_code(ACC_PUBLIC, &name, &inst_desc, locals.max(1), |asm| {
                    asm.aload(0);
                    for (slot, sort) in &loads {
                        load(asm, *slot, *sort);
                    }
                    asm.invokestatic_interface(&iface_c, &static_name, &static_desc);
                    emit_return(asm, &ret);
                });
            }
        }
    }

    /// [`Gen::superclass_implements`], for a member known by symbol rather
    /// than by tree.
    pub(crate) fn superclass_implements_sym(
        &self,
        super_impls: &[(String, Vec<Type>, SymbolId)],
        mid: SymbolId,
    ) -> bool {
        let enc = encode_method_name(&self.st.get(mid).name);
        let declared = method_params_from_sym(self.st, mid);
        let abstract_mask = self
            .st
            .erased_abstract_params
            .get(&mid)
            .copied()
            .unwrap_or(0);
        super_impls.iter().any(|(n, cps, sym)| {
            *n == enc && *sym != mid && bridge_overrides(self.st, &declared, cps, abstract_mask)
        })
    }

    /// Implement the capture accessors every mixed-in *local* trait declares
    /// abstract, reading this class's own capture field. The trait's own
    /// body has nothing but `$this` to reach an enclosing-method local
    /// through; `anon_capture` has already made this class capture whatever
    /// its traits capture, so the field is here.
    pub(crate) fn emit_trait_capture_accessors(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        lin: &[SymbolId],
    ) {
        // Not gated on this class having captures of its own: if a mixed-in
        // trait captures and this class somehow did not, the accessor still
        // has to be *declared* here — see the `else` arm below, which says so
        // out loud rather than leaving an abstract method behind.
        let slots = capture_slots(self.st, &self.boxed_vars, class_id);
        let class_name = b.this_name.clone();
        let mut done: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        for parent in lin.iter().skip(1) {
            if !is_interface_sym(self.st, *parent) {
                continue;
            }
            for (id, aname, adesc, sort) in
                trait_capture_accessors(self.st, &self.boxed_vars, *parent)
            {
                if !done.insert(aname.clone()) {
                    continue;
                }
                let Some((_, fname, fdesc, _)) = slots.iter().find(|s| s.0 == id) else {
                    // `anon_capture` propagates a trait's captures to every
                    // class mixing it in, so this cannot happen; a missing
                    // field would be a silently wrong read, so say so.
                    let msg = format!(
                        "cannot capture {} for trait {}",
                        self.st.get(id).name,
                        self.st.get(*parent).name
                    );
                    b.add_code(ACC_PUBLIC, &aname, &adesc, 1, |asm| {
                        throw_runtime(asm, &msg);
                    });
                    continue;
                };
                let (cn, fname, fdesc) = (class_name.clone(), fname.clone(), fdesc.clone());
                b.add_code(ACC_PUBLIC, &aname, &adesc, 1, move |asm| {
                    asm.aload(0);
                    asm.getfield(&cn, &fname, &fdesc);
                    ret_of_sort(asm, sort);
                });
            }
        }
    }

    /// nsc-style erasure bridges: `compare(that: Box)` does not satisfy
    /// `Ordered.compare(Object)`. Emit a public bridge that checkcasts.
    /// A case class's `toString` / `equals` / `hashCode` / `canEqual`. nsc
    /// synthesizes these from the constructor fields; a hand-written one wins.
    /// `hashCode` folds with 31 rather than nsc's MurmurHash3, so it agrees
    /// with `equals` without depending on `scala.runtime`.
    pub(crate) fn emit_case_object_methods(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() || !self.st.get(class_id).flags.contains(Flags::CASE) {
            return;
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        let class_jvm = b.this_name.clone();
        let simple = self.st.get(class_id).name.clone();
        let defined: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        if is_module_class(self.st, class_id) {
            // The module class is `Asc$`; the `case object` is called `Asc`.
            let name = simple.strip_suffix('$').unwrap_or(&simple).to_string();
            Self::emit_case_module_methods(b, &name, &defined);
            // A `case object` is a zero-field `Product`: every index is out of
            // range, and `productIterator` is empty.
            emit_product_accessors(b, &defined, &class_jvm, &[], &[], self.library_abi, true);
            return;
        }
        let field_info: Vec<(String, Type, String)> = fields
            .iter()
            .map(|f| {
                let s = self.st.get(*f);
                let ty = s.ty.clone();
                let desc = jvm_desc_val(self.st, &ty);
                (s.name.clone(), ty, desc)
            })
            .collect();
        // A field of value-class type is stored unboxed but *printed* as an
        // instance: nsc's `Box(Meters@3,b)`, not `Box(3,b)`.
        let field_vc: Vec<Option<(String, String)>> = fields
            .iter()
            .map(|f| {
                let c = *self.st.value_class_terms.get(f)?;
                let under = self.st.value_class_underlying(c)?;
                Some((
                    class_internal(self.st, c),
                    format!("({})V", jvm_desc(self.st, &under)),
                ))
            })
            .collect();

        if !defined.contains("productPrefix") {
            let text = simple.clone();
            b.add_code(
                ACC_PUBLIC,
                "productPrefix",
                "()Ljava/lang/String;",
                1,
                move |asm| {
                    asm.ldc_string(&text);
                    asm.areturn();
                },
            );
        }
        if !defined.contains("productArity") {
            let n = field_info.len() as i32;
            b.add_code(ACC_PUBLIC, "productArity", "()I", 1, move |asm| {
                asm.iconst(n);
                asm.ireturn();
            });
        }
        emit_product_accessors(
            b,
            &defined,
            &class_jvm,
            &field_info,
            &field_vc,
            self.library_abi,
            false,
        );

        if !defined.contains("toString") {
            let fi = field_info.clone();
            let fvc = field_vc.clone();
            let cj = class_jvm.clone();
            let head = format!("{simple}(");
            b.add_code(ACC_PUBLIC, "toString", "()Ljava/lang/String;", 1, |asm| {
                asm.new_obj("java/lang/StringBuilder");
                asm.dup();
                asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
                append_str(asm, &head);
                for (i, (name, ty, desc)) in fi.iter().enumerate() {
                    if i > 0 {
                        append_str(asm, ",");
                    }
                    if let Some(Some((internal, ctor))) = fvc.get(i) {
                        asm.new_obj(internal);
                        asm.dup();
                        asm.aload(0);
                        asm.getfield(&cj, name, desc);
                        asm.invokespecial(internal, "<init>", ctor);
                        asm.invokevirtual(
                            "java/lang/StringBuilder",
                            "append",
                            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
                        );
                        continue;
                    }
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    let ad = append_desc(ty);
                    if ad == "(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
                        && is_jvm_primitive(ty)
                        && !erases_to_boxed_unit(ty)
                    {
                        emit_box(asm, ty);
                    }
                    asm.invokevirtual("java/lang/StringBuilder", "append", ad);
                }
                append_str(asm, ")");
                asm.invokevirtual(
                    "java/lang/StringBuilder",
                    "toString",
                    "()Ljava/lang/String;",
                );
                asm.areturn();
            });
        }

        if !defined.contains("canEqual") {
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "canEqual", "(Ljava/lang/Object;)Z", 2, |asm| {
                asm.aload(1);
                asm.instanceof(&cj);
                asm.ireturn();
            });
        }

        if !defined.contains("equals") {
            let fi = field_info.clone();
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "equals", "(Ljava/lang/Object;)Z", 3, |asm| {
                let yes = asm.fresh_label();
                let no = asm.fresh_label();
                asm.aload(0);
                asm.aload(1);
                asm.if_acmpeq(yes);
                asm.aload(1);
                asm.instanceof(&cj);
                asm.ifeq(no);
                asm.aload(1);
                asm.checkcast(&cj);
                asm.astore(2);
                for (name, ty, desc) in &fi {
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    asm.aload(2);
                    asm.getfield(&cj, name, desc);
                    match ty {
                        Type::Long => {
                            asm.lcmp();
                            asm.ifne(no);
                        }
                        Type::Double => {
                            asm.dcmpl();
                            asm.ifne(no);
                        }
                        Type::Float => {
                            asm.fcmpl();
                            asm.ifne(no);
                        }
                        // A `Unit` field is a `BoxedUnit` reference, not an
                        // int-sorted primitive: `if_icmpeq` on two references
                        // is a `VerifyError`.
                        t if is_jvm_primitive(t) && !erases_to_boxed_unit(t) => {
                            let eq = asm.fresh_label();
                            asm.if_icmpeq(eq);
                            asm.goto(no);
                            asm.mark(eq);
                        }
                        _ => {
                            asm.invokestatic(
                                "java/util/Objects",
                                "equals",
                                "(Ljava/lang/Object;Ljava/lang/Object;)Z",
                            );
                            asm.ifeq(no);
                        }
                    }
                }
                // SLS 5.3.2 / nsc: the last conjunct is `that.canEqual(this)`,
                // which is what lets a subclass refuse an equality its
                // superclass's fields would otherwise accept. Leaving it out
                // made `case class C1(x: Int)` equal to a subclass that
                // overrides `canEqual` to say no (`run/caseClassEquality`).
                asm.aload(2);
                asm.aload(0);
                asm.invokevirtual(&cj, "canEqual", "(Ljava/lang/Object;)Z");
                asm.ifeq(no);
                asm.mark(yes);
                asm.iconst(1);
                asm.ireturn();
                asm.mark(no);
                asm.iconst(0);
                asm.ireturn();
            });
        }

        if !defined.contains("hashCode") {
            let fi = field_info.clone();
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "hashCode", "()I", 2, |asm| {
                asm.iconst(0);
                for (name, ty, desc) in &fi {
                    asm.iconst(31);
                    asm.imul();
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    if is_jvm_primitive(ty) && !erases_to_boxed_unit(ty) {
                        emit_box(asm, ty);
                    }
                    asm.invokestatic("java/util/Objects", "hashCode", "(Ljava/lang/Object;)I");
                    asm.iadd();
                }
                asm.ireturn();
            });
        }
    }

    /// A `case object`'s synthetic members. nsc gives the module class a
    /// `toString`/`productPrefix` returning the object's name and a `hashCode`
    /// folded at compile time to `name.hashCode`; `equals` stays `Object`'s
    /// reference equality, which is exactly right for a singleton.
    pub(crate) fn emit_case_module_methods(
        b: &mut ClassBuilder,
        name: &str,
        defined: &HashSet<String>,
    ) {
        let class_jvm = b.this_name.clone();
        for m in ["toString", "productPrefix"] {
            if defined.contains(m) {
                continue;
            }
            let text = name.to_string();
            b.add_code(ACC_PUBLIC, m, "()Ljava/lang/String;", 1, move |asm| {
                asm.ldc_string(&text);
                asm.areturn();
            });
        }
        if !defined.contains("hashCode") {
            let h = java_string_hash(name);
            b.add_code(ACC_PUBLIC, "hashCode", "()I", 1, move |asm| {
                asm.iconst(h);
                asm.ireturn();
            });
        }
        if !defined.contains("productArity") {
            b.add_code(ACC_PUBLIC, "productArity", "()I", 1, |asm| {
                asm.iconst(0);
                asm.ireturn();
            });
        }
        if !defined.contains("canEqual") {
            b.add_code(ACC_PUBLIC, "canEqual", "(Ljava/lang/Object;)Z", 2, |asm| {
                asm.aload(1);
                asm.instanceof(&class_jvm);
                asm.ireturn();
            });
        }
    }

    // (helper `desc_param_sorts` is a free function below)

    /// `narrow` is a reference type the JVM would accept where `wide` is
    /// asked for -- the shape a parameter bridge exists to cast across.
    ///
    /// Only reference types, and only when the symbol table really has both
    /// classes and one is above the other: a bridge that casts to an
    /// unrelated class would turn a linkage error into a
    /// `ClassCastException`.
    pub(crate) fn desc_narrows(&self, wide: &str, narrow: &str) -> bool {
        if wide == narrow {
            return true;
        }
        if !wide.starts_with('L') || !narrow.starts_with('L') {
            return false;
        }
        let w = &wide[1..wide.len() - 1];
        let n = &narrow[1..narrow.len() - 1];
        if w == "java/lang/Object" {
            return true;
        }
        let (Some(ws), Some(ns)) = (self.st.find_class_by_jvm(w), self.st.find_class_by_jvm(n))
        else {
            return false;
        };
        let nt = Type::Class {
            sym: ns,
            args: vec![],
        };
        self.st
            .base_type_seq(&nt)
            .iter()
            .any(|p| self.st.class_sym_of(p) == Some(ws))
    }

    /// Covariant-override bridges for members this class only *inherits*.
    ///
    /// `emit_erasure_bridges` looks at the class's own members, so it never
    /// sees an override that happened two traits up: slick's
    /// `RelationalTypesComponent` declares
    /// `def MappedColumnType: MappedColumnTypeFactory` and `JdbcProfile`
    /// overrides it with `override lazy val MappedColumnType:
    /// MappedJdbcType.type`. `H2Profile$` gets the narrow mixin forwarder and
    /// nothing else, so calling the member through the base interface threw
    /// `AbstractMethodError`. nsc emits the bridge on the implementing class;
    /// do the same, for every inherited method whose erased descriptor we do
    /// not implement but whose parameters match one we do.
    pub(crate) fn emit_inherited_covariant_bridges(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
    ) {
        if class_id.is_none() || is_interface_sym(self.st, class_id) {
            return;
        }
        let class_name = b.this_name.clone();
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            for pmid in self.st.get(parent).members.clone() {
                let ps = self.st.get(pmid);
                if ps.name == "<init>"
                    || ps.name == "<clinit>"
                    || ps.flags.contains(Flags::STATIC)
                    || ps.flags.contains(Flags::PARAM)
                {
                    continue;
                }
                // A `val` / `lazy val` is reached through its getter, so it
                // needs the bridge just as much as a `def` does -- in a
                // *class* parent as much as in a trait one. slick's
                // `JdbcStatementBuilderComponent.QueryBuilder` declares
                // `protected val quotedJdbcFns: Option[Seq[JdbcFunction]]` and
                // `H2Profile`'s subclass narrows it to `Some[Nil.type]`;
                // without the wide getter the base class's own `expr` read its
                // own field and quoted every JDBC function
                // (`{fn length("NAME")}` where H2 wants `length("NAME")`).
                // A private member is not overridden at all, so it is left
                // alone.
                let pdesc = match ps.kind {
                    SymKind::Method => method_desc_from_sym(self.st, pmid),
                    SymKind::Term if !ps.flags.contains(Flags::PRIVATE) => {
                        format!("(){}", jvm_desc(self.st, &ps.ty))
                    }
                    _ => continue,
                };
                let enc = encode_method_name(&ps.name);
                let Some(cut) = pdesc.find(')') else { continue };
                let (pparams, pret) = (&pdesc[..=cut], &pdesc[cut + 1..]);
                if !(pret.starts_with('L') || pret.starts_with('[')) {
                    continue;
                }
                if b.methods.iter().any(|m| m.name == enc && m.desc == pdesc) {
                    continue;
                }
                let same_params = b
                    .methods
                    .iter()
                    .find(|m| {
                        m.name == enc
                            && m.code.is_some()
                            && m.access & ACC_STATIC == 0
                            && m.desc.starts_with(pparams)
                            && m.desc[pparams.len()..].starts_with('L')
                    })
                    .map(|m| m.desc.clone());
                // The same rule with the parameters narrowed rather than the
                // result. slick's `RelationalActionComponent` declares
                // `createSchemaActionExtensionMethods(_: SchemaDescription)`
                // over an abstract type that `SqlProfile` fixes to
                // `SqlProfile#DDL`, so `H2Profile$` implements only the narrow
                // descriptor and a call through the base interface threw
                // `AbstractMethodError`. nsc's bridge takes the wide
                // descriptor and `checkcast`s each narrowed argument.
                //
                // Only when exactly one implemented method of that name fits:
                // an overload set is `emit_erasure_bridges`' business, which
                // knows which symbol overrides which.
                let pparam_strs = desc_param_strs(pdesc.as_str());
                let have = match same_params {
                    Some(d) => d,
                    None => {
                        let mut fits = b.methods.iter().filter(|m| {
                            m.name == enc
                                && m.code.is_some()
                                && m.access & ACC_STATIC == 0
                                && m.desc[m.desc.find(')').map(|i| i + 1).unwrap_or(0)..]
                                    .starts_with('L')
                                && {
                                    let cs = desc_param_strs(&m.desc);
                                    cs.len() == pparam_strs.len()
                                        && cs
                                            .iter()
                                            .zip(&pparam_strs)
                                            .all(|(c, p)| self.desc_narrows(p, c))
                                }
                        });
                        let Some(first) = fits.next().map(|m| m.desc.clone()) else {
                            continue;
                        };
                        if fits.next().is_some() {
                            continue;
                        }
                        first
                    }
                };
                let cparam_strs = desc_param_strs(&have);
                let mut loads: Vec<(u16, JvmSort, Option<String>)> = Vec::new();
                let mut locals = 1u16;
                for (i, t) in desc_param_sorts(pparams).into_iter().enumerate() {
                    // A narrowed reference parameter is cast to what the
                    // implementation declares; everything else is passed on.
                    let cast = match (pparam_strs.get(i), cparam_strs.get(i)) {
                        (Some(p), Some(c)) if p != c && c.starts_with('L') => {
                            Some(c[1..c.len() - 1].to_string())
                        }
                        (Some(p), Some(c)) if p != c && c.starts_with('[') => Some(c.clone()),
                        _ => None,
                    };
                    loads.push((locals, t, cast));
                    locals += t.slots();
                }
                let cn = class_name.clone();
                let target = have.clone();
                let name = enc.clone();
                b.add_code(
                    ACC_PUBLIC | ACC_BRIDGE | ACC_SYNTHETIC,
                    &enc,
                    &pdesc,
                    locals.max(1),
                    move |asm| {
                        asm.aload(0);
                        for (slot, sort, cast) in &loads {
                            load(asm, *slot, *sort);
                            if let Some(c) = cast {
                                asm.checkcast(c);
                            }
                        }
                        asm.invokevirtual(&cn, &name, &target);
                        asm.areturn();
                    },
                );
            }
        }
    }

    /// Bridges for members inherited from parents that live on the classpath.
    ///
    /// `emit_inherited_covariant_bridges` bridges *to a method this class
    /// implements*, and reads the parents out of the symbol table. Neither
    /// holds for the scala-library collections: nothing in slick names
    /// `iterableFactory`, so the symbol table has never heard of it, and the
    /// anonymous `immutable.IndexedSeq` implements nothing but `apply` and
    /// `length`. The member set therefore has to come from the parents' class
    /// files, and the bridge forwards to a *default method*. See
    /// [`crate::ifacebridge`] for what goes wrong without it.
    pub(crate) fn emit_binary_parent_bridges(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        let Some(bp) = self.binary_parents.clone() else {
            return;
        };
        if b.access & ACC_INTERFACE != 0 {
            return;
        }
        // Seed the walk with the whole linearization, not just the class
        // file's direct parents: a trait compiled in this same run is not on
        // the binary path, and stopping there would hide the library
        // ancestors above it.
        let mut roots: Vec<String> = Vec::new();
        if b.super_name != "java/lang/Object" {
            roots.push(b.super_name.clone());
        }
        roots.extend(b.interfaces.iter().cloned());
        if !class_id.is_none() {
            for p in linearize(self.st, class_id).into_iter().skip(1) {
                roots.push(class_internal(self.st, p));
            }
        }
        let have: HashSet<(String, String)> = b
            .methods
            .iter()
            .map(|m| (m.name.clone(), m.desc.clone()))
            .collect();
        let class_name = b.this_name.clone();
        for br in bp.bridges(&roots, &have) {
            let Some(cut) = br.desc.find(')') else {
                continue;
            };
            let ret = br.desc[cut + 1..].to_string();
            let mut loads: Vec<(u16, JvmSort)> = Vec::new();
            let mut locals = 1u16;
            for t in desc_param_sorts(&br.desc[..=cut]) {
                loads.push((locals, t));
                locals += t.slots();
            }
            let cn = class_name.clone();
            let name = br.name.clone();
            let kind = br.kind.clone();
            // A covariant bridge is `ACC_BRIDGE`; a mixin forwarder that is
            // the only implementation the class has is an ordinary method, as
            // nsc emits it.
            let access = match kind {
                BridgeKind::Narrow(_) => ACC_PUBLIC | ACC_BRIDGE | ACC_SYNTHETIC,
                BridgeKind::Static { .. } => ACC_PUBLIC,
            };
            b.add_code(access, &br.name, &br.desc, locals.max(1), move |asm| {
                match &kind {
                    BridgeKind::Narrow(target) => {
                        asm.aload(0);
                        for (slot, sort) in &loads {
                            load(asm, *slot, *sort);
                        }
                        asm.invokevirtual(&cn, &name, target);
                    }
                    BridgeKind::Static {
                        iface,
                        helper,
                        desc,
                    } => {
                        asm.aload(0);
                        for (slot, sort) in &loads {
                            load(asm, *slot, *sort);
                        }
                        asm.invokestatic_interface(iface, helper, desc);
                    }
                }
                ret_of_sort(asm, ret_str_sort(&ret));
            });
        }
    }

    pub(crate) fn emit_erasure_bridges(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let class_name = b.this_name.clone();
        let existing: HashSet<(String, String)> = b
            .methods
            .iter()
            .map(|m| (m.name.clone(), m.desc.clone()))
            .collect();
        let own: Vec<(String, SymbolId)> = self
            .st
            .get(class_id)
            .members
            .iter()
            .copied()
            .filter(|&id| self.st.get(id).kind == SymKind::Method)
            .map(|id| (self.st.get(id).name.clone(), id))
            .collect();
        let lin = linearize(self.st, class_id);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for parent in lin.into_iter().skip(1) {
            for pmid in self.st.get(parent).members.clone() {
                let ps = self.st.get(pmid);
                if ps.kind != SymKind::Method {
                    continue;
                }
                if ps.name == "<init>" || ps.name == "<clinit>" {
                    continue;
                }
                // Among the class's own alternatives of that name, the one
                // that overrides this parent method -- not just the first one
                // spelled the same way.
                let parent_params = method_params_from_sym(self.st, pmid);
                let parent_abstract = self
                    .st
                    .erased_abstract_params
                    .get(&pmid)
                    .copied()
                    .unwrap_or(0);
                let Some((_, cid)) = own.iter().find(|(n, id)| {
                    n == &ps.name
                        && *id != pmid
                        && bridge_overrides(
                            self.st,
                            &parent_params,
                            &method_params_from_sym(self.st, *id),
                            parent_abstract,
                        )
                }) else {
                    continue;
                };
                if *cid == pmid {
                    continue;
                }
                let pdesc = method_desc_from_sym(self.st, pmid);
                let cdesc = method_desc_from_sym(self.st, *cid);
                if pdesc == cdesc {
                    continue;
                }
                let enc = encode_method_name(&ps.name);
                if existing.contains(&(enc.clone(), pdesc.clone())) {
                    continue;
                }
                if !seen.insert((enc.clone(), pdesc.clone())) {
                    continue;
                }
                let child_params = method_params_from_sym(self.st, *cid);
                let ret = method_ret_from_sym(self.st, pmid);
                let child_ret = method_ret_from_sym(self.st, *cid);
                // The bridge takes the erased parent signature, so a parameter
                // the subclass narrowed to a primitive arrives boxed.
                let ret_adapt = if jvm_desc(self.st, &ret) == jvm_desc(self.st, &child_ret) {
                    Adapt::None
                } else {
                    param_adapt(self.st, &child_ret, &ret)
                };
                // A `Unit` result is `V` in the implementation's own
                // descriptor -- the call leaves nothing on the stack -- while
                // the bridge owes a reference. nsc pushes `BoxedUnit.UNIT`
                // there. `param_adapt`'s `Unit` rule is the *parameter* one (a
                // `Unit` argument really does arrive as a `BoxedUnit`
                // reference) and has nothing to say about the result:
                // `object SetUnit extends SetParameter[Unit]`, whose
                // `SetParameter[-T] extends ((T, PositionedParameters) =>
                // Unit)`, got `invokevirtual apply(…)V; areturn` --
                // `VerifyError: Operand stack underflow`.
                let fill_unit = cdesc.ends_with(")V") && !pdesc.ends_with(")V");
                let mut locals = 1u16;
                let mut loads = Vec::new();
                let mut casts: Vec<Adapt> = Vec::new();
                for (pty, cty) in parent_params.iter().zip(child_params.iter()) {
                    let sort = jvm_slot_sort(pty);
                    loads.push((locals, sort));
                    let adapt = if jvm_desc(self.st, pty) != jvm_desc(self.st, cty) {
                        param_adapt(self.st, pty, cty)
                    } else {
                        Adapt::None
                    };
                    casts.push(adapt);
                    locals += sort.slots();
                }
                let name = ps.name.clone();
                let pdesc_c = pdesc.clone();
                let cdesc_c = cdesc.clone();
                let class_c = class_name.clone();
                b.add_code(
                    ACC_PUBLIC | ACC_SYNTHETIC | ACC_BRIDGE,
                    &name,
                    &pdesc_c,
                    locals.max(1),
                    |asm| {
                        asm.aload(0);
                        for (i, (slot, sort)) in loads.iter().enumerate() {
                            load(asm, *slot, *sort);
                            if let Some(a) = casts.get(i) {
                                emit_adapt(asm, a);
                            }
                        }
                        asm.invokevirtual(&class_c, &name, &cdesc_c);
                        if fill_unit {
                            emit_boxed_unit(asm);
                        }
                        emit_adapt(asm, &ret_adapt);
                        emit_return(asm, &ret);
                    },
                );
            }
        }
    }

    pub(crate) fn emit_ordered_forwarders(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        defined: &HashSet<String>,
    ) {
        if class_id.is_none() {
            return;
        }
        let lin = linearize(self.st, class_id);
        let has_ordered = lin.iter().any(|&p| self.st.get(p).name == "Ordered");
        if !has_ordered {
            return;
        }
        for op in ["<", ">", "<=", ">="] {
            let enc = encode_method_name(op);
            if defined.contains(op) || defined.contains(&enc) {
                continue;
            }
            let desc = "(Ljava/lang/Object;)Z";
            let static_desc = "(Lscala/math/Ordered;Ljava/lang/Object;)Z";
            let name = op.to_string();
            let static_name = trait_static_name(op);
            b.add_code(ACC_PUBLIC, &name, desc, 2, |asm| {
                asm.aload(0);
                asm.aload(1);
                asm.invokestatic_interface("scala/math/Ordered", &static_name, static_desc);
                asm.ireturn();
            });
        }
    }

    /// `(encoded name, descriptor)` of every `name$default$n` getter `owner`
    /// declares, computed exactly as `emit_default_getters` does.
    pub(crate) fn default_getter_sigs(&self, owner: SymbolId) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for mid in self.st.get(owner).members.clone() {
            let s = self.st.get(mid);
            if s.kind != SymKind::Method || !s.name.contains("$default$") {
                continue;
            }
            if s.default_rhs.is_none() {
                continue;
            }
            let pts: Vec<Type> = if !s.params.is_empty() {
                s.params
                    .iter()
                    .map(|p| self.st.get(*p).ty.clone())
                    .collect()
            } else {
                match &s.ty {
                    Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
                    _ => vec![],
                }
            };
            let ret = match &s.ty {
                Type::Method { ret, .. } => (**ret).clone(),
                _ => Type::Any,
            };
            out.push((
                encode_method_name(&s.name),
                jvm_method_desc(self.st, &pts, &ret),
            ));
        }
        out
    }

    pub(crate) fn emit_default_getters(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let mut existing: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        // The whole linearization, not just this class's own members: a
        // *trait* method with a defaulted parameter declares its
        // `name$default$n` getter on the interface (see the trait arm of
        // `emit_class`), and every implementing class owes a body. slick calls
        // `n.mapChildren(f)` on the `Node` trait from another file, and the
        // omitted `keepType` argument became
        // `NoSuchMethodError: Node.mapChildren$default$2`.
        let mut ids: Vec<SymbolId> = self.st.get(class_id).members.clone();
        for p in linearize(self.st, class_id).into_iter().skip(1) {
            if is_interface_sym(self.st, p) {
                ids.extend(self.st.get(p).members.clone());
            }
        }
        for mid in ids {
            let s = self.st.get(mid);
            if s.kind != SymKind::Method || !s.name.contains("$default$") {
                continue;
            }
            if !existing.insert(encode_method_name(&s.name)) {
                continue;
            }
            let Some(rhs) = s.default_rhs.clone() else {
                continue;
            };
            let rhs2 = rhs.clone();
            let name = s.name.clone();
            let pts: Vec<Type> = if !s.params.is_empty() {
                s.params
                    .iter()
                    .map(|p| self.st.get(*p).ty.clone())
                    .collect()
            } else {
                match &s.ty {
                    Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
                    _ => vec![],
                }
            };
            let ret = match &s.ty {
                Type::Method { ret, .. } => (**ret).clone(),
                _ => rhs.ty.clone(),
            };
            let desc = jvm_method_desc(self.st, &pts, &ret);
            let mut frame = Frame::instance();
            let pids = s.params.clone();
            for (i, ty) in pts.iter().enumerate() {
                let id = pids.get(i).copied().unwrap_or(SymbolId::NONE);
                frame.alloc_param(id, jvm_sort(ty), ty);
            }
            let class_name = b.this_name.clone();
            let st = self.st;
            let max_locals = frame.next_slot.max(1);
            let extras = &self.extras;
            let lambda_n = &self.lambda_n;
            let lambda_bodies = &self.lambda_bodies;
            let hoist_owner = b.this_name.clone();
            let source = self.source_name;
            let library_abi = self.library_abi;
            let boxed_vars = &self.boxed_vars;
            let ret_for_body = ret.clone();
            b.add_code(
                ACC_PUBLIC | ACC_SYNTHETIC,
                &name,
                &desc,
                max_locals,
                |asm| {
                    let mut frame = frame;
                    let ctx = emit_ctx(
                        st,
                        class_id,
                        &class_name,
                        ret_for_body.clone(),
                        extras,
                        lambda_n,
                        lambda_bodies,
                        Some(&hoist_owner),
                        source,
                        library_abi,
                        boxed_vars,
                    );
                    gen_expr(asm, &mut frame, &ctx, &rhs);
                    if is_unit_like(&ret_for_body) {
                        pop_if_value(asm, &rhs.ty);
                        asm.vreturn();
                    } else {
                        emit_return(asm, &ret_for_body);
                    }
                },
            );
            // A value class's methods are called through their `$extension`
            // statics, and so are the default getters that go with them:
            // slick's `NodeOps.collect(pf, stopOnMatch = false)` compiled to
            // `collect$default$2$extension(Node, PartialFunction)Z`, which
            // nothing emitted.
            let Some(under) = self.st.value_class_underlying(class_id) else {
                continue;
            };
            let ext_name = format!("{}$extension", name);
            if !existing.insert(ext_name.clone()) {
                continue;
            }
            let mut tys = vec![under.clone()];
            tys.extend(pts.iter().cloned());
            let ext_desc = jvm_method_desc(self.st, &tys, &ret);
            let mut frame = Frame {
                locals: HashMap::new(),
                next_slot: 0,
                finally_exits: Vec::new(),
                return_slot: None,
            };
            match self.st.get(class_id).ctor_fields.first().copied() {
                Some(fid) => {
                    frame.alloc(fid, jvm_sort(&under));
                }
                None => frame.next_slot = 1,
            }
            for (i, ty) in pts.iter().enumerate() {
                let id = pids.get(i).copied().unwrap_or(SymbolId::NONE);
                frame.alloc_param(id, jvm_sort(ty), ty);
            }
            let class_name = b.this_name.clone();
            let ret_for_body = ret.clone();
            let max_locals = frame.next_slot.max(1);
            let under_c = under.clone();
            b.add_code(
                ACC_PUBLIC | ACC_STATIC | ACC_SYNTHETIC,
                &ext_name,
                &ext_desc,
                max_locals,
                |asm| {
                    let mut frame = frame;
                    let mut ctx = emit_ctx(
                        st,
                        class_id,
                        &class_name,
                        ret_for_body.clone(),
                        extras,
                        lambda_n,
                        lambda_bodies,
                        Some(&hoist_owner),
                        source,
                        library_abi,
                        boxed_vars,
                    );
                    ctx.value_ext = Some((
                        class_name.clone(),
                        format!("({})V", jvm_desc_val(st, &under_c)),
                        jvm_sort(&under_c),
                    ));
                    gen_expr(asm, &mut frame, &ctx, &rhs2);
                    if is_unit_like(&ret_for_body) {
                        pop_if_value(asm, &rhs2.ty);
                        asm.vreturn();
                    } else {
                        emit_return(asm, &ret_for_body);
                    }
                },
            );
        }
    }

    /// The member `object`s a template holds itself: `class Outer { object P }`
    /// keeps `P`'s single instance in a `private volatile P$module` field on
    /// `Outer` and hands it out through a `P()` accessor that creates it on
    /// first use, exactly as nsc's `lazyValNullables`/mixin phases emit it.
    pub(crate) fn member_modules_of(&self, template: SymbolId, body: &[Tree]) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for s in body {
            // A `case class` nested here carries a synthetic companion that is
            // nested too, and needs the same accessor even though there is no
            // `ModuleDef` for it.
            let mcls = match &s.kind {
                TreeKind::ModuleDef { .. } if !s.sym.is_none() => module_class_id(self.st, s.sym),
                TreeKind::ClassDef { mods, .. }
                    if mods.flags.contains(Flags::CASE) && !s.sym.is_none() =>
                {
                    match self.st.companion_module(s.sym) {
                        Some(m) => module_class_id(self.st, m),
                        None => continue,
                    }
                }
                _ => continue,
            };
            if member_module_outer(self.st, mcls) == Some(template) && !out.contains(&mcls) {
                out.push(mcls);
            }
        }
        out
    }

    /// Field + accessor for every member `object` a concrete template has to
    /// hold — its own, and the ones inherited from mixed-in traits, whose
    /// accessor is an interface method the class has to implement.
    pub(crate) fn emit_member_module_accessors(&self, b: &mut ClassBuilder, modules: &[SymbolId]) {
        for &mcls in modules {
            let this_name = b.this_name.clone();
            let mjvm = class_internal(self.st, mcls);
            let mdesc = format!("L{mjvm};");
            let fname = module_field_name(self.st, mcls);
            let aname = module_accessor_name(self.st, mcls);
            let adesc = module_accessor_desc(self.st, mcls);
            b.fields.push(Field {
                access: ACC_PRIVATE | ACC_VOLATILE,
                name: fname.clone(),
                desc: mdesc.clone(),
            });
            // The enclosing instance the module's `<init>` wants is typed by
            // `outer_field_desc`; for a cake component that is the self type,
            // so pass `this` through the same cast the field expects.
            let ctor_desc = outer_field_desc(self.st, mcls)
                .map(|d| format!("({d})V"))
                .unwrap_or_else(|| format!("(L{this_name};)V"));
            let cast_to = outer_field_class(self.st, mcls)
                .filter(|o| class_internal(self.st, *o) != this_name)
                .map(|o| class_internal(self.st, o));
            b.add_code(ACC_PUBLIC, &aname, &adesc, 3, |asm| {
                asm.aload(0);
                asm.getfield(&this_name, &fname, &mdesc);
                let done = asm.fresh_label();
                asm.ifnonnull(done);
                let lock = 1u16;
                asm.aload(0);
                asm.dup();
                asm.astore(lock);
                asm.monitorenter();
                asm.aload(0);
                asm.getfield(&this_name, &fname, &mdesc);
                let made = asm.fresh_label();
                asm.ifnonnull(made);
                asm.aload(0);
                asm.new_obj(&mjvm);
                asm.dup();
                asm.aload(0);
                if let Some(c) = &cast_to {
                    asm.checkcast(c);
                }
                asm.invokespecial(&mjvm, "<init>", &ctor_desc);
                asm.putfield(&this_name, &fname, &mdesc);
                asm.mark(made);
                asm.aload(lock);
                asm.monitorexit();
                asm.mark(done);
                asm.aload(0);
                asm.getfield(&this_name, &fname, &mdesc);
                asm.areturn();
            });
        }
    }

    /// Implement `<Trait>$$$outer()` for every mixed-in trait that is nested
    /// in a class. nsc's mixin phase puts one on each implementing class; the
    /// trait's own code calls it instead of reading a field it cannot have.
    pub(crate) fn emit_trait_outer_accessors(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let mut done: HashSet<String> = HashSet::new();
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            if !is_interface_sym(self.st, parent) {
                continue;
            }
            let Some(o) = outer_field_class(self.st, parent) else {
                continue;
            };
            // A trait nested in a member `object` is reached through that
            // object's accessor, not along the `$outer` chain. slick has
            // `trait JdbcStatementBuilderComponent { object TableDDLBuilder {
            // trait UniqueIndexAsConstraint extends TableDDLBuilder } }`, and
            // `class H2TableDDLBuilder extends TableDDLBuilder(table) with
            // TableDDLBuilder.UniqueIndexAsConstraint` holds an `$outer` of
            // `H2Profile` -- the object itself is nowhere on that chain, so
            // this declined to implement the accessor at all and the JVM threw
            // `AbstractMethodError` at the first `createIndex`.
            // `H2Profile.TableDDLBuilder()` is the instance, which is what
            // `load_module_instance` reaches.
            let via_module = member_module_outer(self.st, o)
                .is_some_and(|m| outer_chain_reaches(self.st, class_id, m));
            // A trait nested in a *trait* (`trait T { trait NT }`) mixed into a
            // class that is not itself nested: `object Test extends T { new NT
            // {} }`. The anonymous class has no `$outer` field and nothing on
            // its (empty) chain conforms to `T`, but the enclosing module
            // *is* a `T`, which is what nsc returns from
            // `T$NT$$$outer()`. Without this the interface's accessor stayed
            // unimplemented and the trait's own `$init$` threw
            // `AbstractMethodError` on the first instantiation.
            let mut via_enclosing = SymbolId::NONE;
            if !via_module && !outer_chain_reaches(self.st, class_id, o) {
                match enclosing_module_conforming(self.st, class_id, o) {
                    Some(m) => via_enclosing = m,
                    None => continue,
                }
            }
            let name = trait_outer_accessor_name(self.st, parent);
            if !done.insert(name.clone()) {
                continue;
            }
            let desc = format!("()L{};", class_internal(self.st, o));
            let class_name = b.this_name.clone();
            let st = self.st;
            let extras = &self.extras;
            let lambda_n = &self.lambda_n;
            let lambda_bodies = &self.lambda_bodies;
            let hoist_owner = b.this_name.clone();
            let source = self.source_name;
            let library_abi = self.library_abi;
            let boxed_vars = &self.boxed_vars;
            b.add_code(ACC_PUBLIC, &name, &desc, 2, |asm| {
                let ctx = emit_ctx(
                    st,
                    class_id,
                    &class_name,
                    Type::Unit,
                    extras,
                    lambda_n,
                    lambda_bodies,
                    Some(&hoist_owner),
                    source,
                    library_abi,
                    boxed_vars,
                );
                if via_module {
                    load_module_instance(asm, &ctx, o);
                } else if !via_enclosing.is_none() {
                    load_module_instance(asm, &ctx, via_enclosing);
                    if !is_owner_compatible(st, via_enclosing, o) {
                        asm.checkcast(&class_internal(st, o));
                    }
                } else {
                    load_owner_instance(asm, &ctx, o);
                }
                asm.areturn();
            });
        }
    }

    /// `lazies` is the class's complete list of `lazy val`s — its own and the
    /// ones inherited from mixed-in traits — so bits are unique. Bit `n` lives
    /// in `bitmap$(n / 32)`; `lazy_bitmap_fields` declares the same words.
    pub(crate) fn emit_lazy_accessors(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        lazies: &[Tree],
        binary: &[BinaryLazyVal],
    ) {
        // The class's own `lazy val`s and the ones inherited from *source*
        // traits carry their initialiser as a tree; one inherited from a trait
        // that arrived as a class file is a call to that trait's `d$` static,
        // which is where nsc put the initialiser. Both share the bitmap words.
        let mut items: Vec<(String, Type, LazyInit)> = Vec::new();
        for stt in lazies {
            let TreeKind::ValDef {
                name, mods, rhs, ..
            } = &stt.kind
            else {
                continue;
            };
            if !mods.flags.contains(Flags::LAZY) || rhs.is_empty() {
                continue;
            }
            let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                self.st.get(stt.sym).ty.clone()
            } else {
                stt.ty.clone()
            };
            items.push((name.clone(), ty, LazyInit::Rhs(rhs.clone())));
        }
        for v in binary {
            let iface = class_internal(self.st, v.owner);
            let getter = format!("(){}", jvm_desc(self.st, &v.ty));
            items.push((
                v.name.clone(),
                v.ty.clone(),
                LazyInit::TraitStatic {
                    static_desc: trait_static_desc(&iface, &getter),
                    static_name: trait_static_name(&v.name),
                    iface,
                },
            ));
        }
        for (bit, (name, ty, init)) in items.into_iter().enumerate() {
            let name = &name;
            let desc = format!("(){}", jvm_desc(self.st, &ty));
            let class_name = b.this_name.clone();
            let fname = name.clone();
            let fdesc = jvm_desc_val(self.st, &ty);
            let st = self.st;
            let extras = &self.extras;
            let lambda_n = &self.lambda_n;
            let lambda_bodies = &self.lambda_bodies;
            let hoist_owner = b.this_name.clone();
            let source = self.source_name;
            let library_abi = self.library_abi;
            let boxed_vars = &self.boxed_vars;
            let mask = 1i32 << (bit % 32);
            let bitmap = format!("bitmap${}", bit / 32);
            let ret_ty = ty.clone();
            let caps = capture_slots(self.st, &self.boxed_vars, class_id);
            b.add_code(ACC_PUBLIC, &fname, &desc, 4, |asm| {
                let mut frame = Frame::instance();
                emit_capture_prologue(asm, &mut frame, &class_name, &caps);
                let lock = frame.alloc_tmp(JvmSort::Ref);
                let result = frame.alloc_tmp(jvm_sort(&ret_ty));
                asm.aload(0);
                store(asm, lock, JvmSort::Ref);
                // Stored before the guarded range so the handler's stack map
                // does not claim a local the body has not written yet.
                push_default(asm, &ret_ty);
                store(asm, result, jvm_sort(&ret_ty));
                load(asm, lock, JvmSort::Ref);
                asm.monitorenter();
                asm.capture_try_locals();
                let try_s = asm.fresh_label();
                asm.mark(try_s);
                asm.aload(0);
                asm.getfield(&class_name, &bitmap, "I");
                asm.iconst(mask);
                asm.iand();
                let inited = asm.fresh_label();
                asm.ifne(inited);
                asm.aload(0);
                match &init {
                    LazyInit::Rhs(rhs) => {
                        let ctx = emit_ctx(
                            st,
                            class_id,
                            &class_name,
                            ret_ty.clone(),
                            extras,
                            lambda_n,
                            lambda_bodies,
                            Some(&hoist_owner),
                            source,
                            library_abi,
                            boxed_vars,
                        );
                        gen_expr(asm, &mut frame, &ctx, rhs);
                    }
                    LazyInit::TraitStatic {
                        iface,
                        static_name,
                        static_desc,
                    } => {
                        asm.aload(0);
                        asm.invokestatic_interface(iface, static_name, static_desc);
                    }
                }
                emit_putfield_from_expr(asm, &class_name, &fname, &fdesc);
                asm.aload(0);
                asm.aload(0);
                asm.getfield(&class_name, &bitmap, "I");
                asm.iconst(mask);
                asm.ior();
                asm.putfield(&class_name, &bitmap, "I");
                asm.mark(inited);
                asm.aload(0);
                emit_getfield(asm, &class_name, &fname, &fdesc);
                store(asm, result, jvm_sort(&ret_ty));
                load(asm, lock, JvmSort::Ref);
                asm.monitorexit();
                // An initialiser that throws used to leave the monitor held:
                // HotSpot then reports the *unbalanced lock* on the way out
                // (`IllegalMonitorStateException`) and the real exception is
                // lost. nsc wraps the region in a catch-all that unlocks and
                // rethrows; so does the local-`lazy val` accessor above.
                let try_e = asm.fresh_label();
                asm.mark(try_e);
                let after = asm.fresh_label();
                asm.goto(after);
                let handler = asm.fresh_label();
                asm.mark(handler);
                asm.enter_handler_captured_locals();
                let ex = frame.alloc_tmp(JvmSort::Ref);
                asm.astore(ex);
                load(asm, lock, JvmSort::Ref);
                asm.monitorexit();
                asm.aload(ex);
                asm.athrow();
                asm.exception(try_s, try_e, handler, None);
                asm.release_try_locals();
                asm.mark(after);
                load(asm, result, jvm_sort(&ret_ty));
                emit_return(asm, &ret_ty);
            });
        }
    }

    /// nsc-style val getters (`def Red: Value`) so `scala.Enumeration` reflection
    /// (`populateNameMap` / `isValDef`) can pair method `Red()` with field `Red`.
    /// `class C(val x: Int)` needs the `x()` accessor nsc emits: without it a
    /// constructor `val` cannot implement a trait's abstract `def x`.
    /// A `var` also gets `x_$eq`.
    pub(crate) fn emit_ctor_val_getters(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
    ) {
        if class_id.is_none() {
            return;
        }
        let class_name = b.this_name.clone();
        let defined: HashSet<(String, String)> = b
            .methods
            .iter()
            .map(|m| (m.name.clone(), m.desc.clone()))
            .collect();
        // A case class turns its first parameter list into `val`s even without
        // the keyword, so `case class ConstRep[T](value: T) extends Rep[T]`
        // implements `Rep.value` with the accessor emitted here. nsc rejects a
        // case class whose first list is implicit, so clause 0 is the one.
        let class_is_case = self.st.get(class_id).flags.contains(Flags::CASE);
        for (clause_idx, clause) in vparamss.iter().enumerate() {
            for p in clause {
                let TreeKind::ValDef { name, mods, .. } = &p.kind else {
                    continue;
                };
                let is_val = mods.flags.contains(Flags::ACCESSOR)
                    || (class_is_case
                        && clause_idx == 0
                        && !mods.flags.contains(Flags::IMPLICIT)
                        && !mods.flags.contains(Flags::MUTABLE));
                let is_var = mods.flags.contains(Flags::MUTABLE);
                if !is_val && !is_var {
                    continue;
                }
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                if ty.is_no_type() || ty.is_error() {
                    continue;
                }
                let fdesc = jvm_desc_val(self.st, &ty);
                let getter = format!("(){}", jvm_desc(self.st, &ty));
                let enc = encode_method_name(name);
                if !defined.contains(&(enc.clone(), getter.clone())) {
                    let fname = name.clone();
                    let cn = class_name.clone();
                    let fd = fdesc.clone();
                    let ret_ty = ty.clone();
                    b.add_code(ACC_PUBLIC, name, &getter, 1, move |asm| {
                        asm.aload(0);
                        emit_getfield(asm, &cn, &fname, &fd);
                        emit_return(asm, &ret_ty);
                    });
                }
                // The parent may declare the member with an erased signature
                // (`def value: T` becomes `value()Object`); bridge to it.
                for parent in linearize(self.st, class_id).into_iter().skip(1) {
                    let Some(pm) = self.st.get(parent).members.iter().copied().find(|&m| {
                        self.st.get(m).kind == SymKind::Method && self.st.get(m).name == *name
                    }) else {
                        continue;
                    };
                    if !method_params_from_sym(self.st, pm).is_empty() {
                        continue;
                    }
                    let pret = method_ret_from_sym(self.st, pm);
                    let pdesc = format!("(){}", jvm_desc(self.st, &pret));
                    if pdesc == getter || defined.contains(&(enc.clone(), pdesc.clone())) {
                        continue;
                    }
                    let cn = class_name.clone();
                    let mname = name.clone();
                    let cdesc = getter.clone();
                    let child_ty = ty.clone();
                    let ret_ty = pret.clone();
                    b.add_code(
                        ACC_PUBLIC | ACC_SYNTHETIC | ACC_BRIDGE,
                        name,
                        &pdesc,
                        1,
                        move |asm| {
                            asm.aload(0);
                            asm.invokevirtual(&cn, &mname, &cdesc);
                            if is_jvm_primitive(&child_ty) && !is_jvm_primitive(&ret_ty) {
                                emit_box(asm, &child_ty);
                            }
                            emit_return(asm, &ret_ty);
                        },
                    );
                    break;
                }
                if is_var {
                    let setter_name = format!("{name}_$eq");
                    let setter = format!("({fdesc})V");
                    if !defined.contains(&(setter_name.clone(), setter.clone())) {
                        let fname = name.clone();
                        let cn = class_name.clone();
                        let fd = fdesc.clone();
                        let sort = jvm_slot_sort(&ty);
                        b.add_code(ACC_PUBLIC, &setter_name, &setter, 3, move |asm| {
                            asm.aload(0);
                            load(asm, 1, sort);
                            asm.putfield(&cn, &fname, &fd);
                            asm.vreturn();
                        });
                    }
                }
            }
        }
    }

    pub(crate) fn emit_val_getters(&self, b: &mut ClassBuilder, body: &[Tree]) {
        let class_name = b.this_name.clone();
        for stt in body {
            let TreeKind::ValDef { name, mods, .. } = &stt.kind else {
                continue;
            };
            if mods.flags.contains(Flags::LAZY) {
                continue;
            }
            let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                self.st.get(stt.sym).ty.clone()
            } else {
                stt.ty.clone()
            };
            if ty.is_no_type() || ty.is_error() {
                continue;
            }
            let desc = format!("(){}", jvm_desc(self.st, &ty));
            let fname = name.clone();
            let fdesc = jvm_desc_val(self.st, &ty);
            let ret_ty = ty.clone();
            let cls = class_name.clone();
            b.add_code(ACC_PUBLIC, &fname, &desc, 1, |asm| {
                asm.aload(0);
                emit_getfield(asm, &cls, &fname, &fdesc);
                emit_return(asm, &ret_ty);
            });
            // A `var` also gets nsc's `v_$eq`; that is the setter an abstract
            // `var` declared in a mixed-in trait resolves to.
            if !mods.flags.contains(Flags::MUTABLE) {
                continue;
            }
            let setter = var_setter_name(name);
            if b.methods.iter().any(|m| m.name == setter) {
                continue;
            }
            let fname = name.clone();
            let fdesc = jvm_desc_val(self.st, &ty);
            let cls = class_name.clone();
            let sort = jvm_slot_sort(&ty);
            b.add_code(
                ACC_PUBLIC,
                &setter,
                &format!("({fdesc})V"),
                1 + sort.slots(),
                |asm| {
                    asm.aload(0);
                    load(asm, 1, sort);
                    asm.putfield(&cls, &fname, &fdesc);
                    asm.vreturn();
                },
            );
        }
    }
}
