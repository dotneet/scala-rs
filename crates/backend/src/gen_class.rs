//! `Gen`'s class-shaped emission: the walk over a compilation unit, the
//! classfile skeleton of a `class`, its primary constructor, `App` and
//! `DelayedInit` support, ordinary method definitions with their Java varargs
//! forwarders, and the `$extension` methods of a value class.

use crate::classfile::{
    Field, ACC_ABSTRACT, ACC_FINAL, ACC_INTERFACE, ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
    ACC_SYNTHETIC, ACC_VARARGS,
};
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::SymKind;
use std::collections::{HashMap, HashSet};

impl<'a> Gen<'a> {
    /// The generic signature recorded for `sym` before erasure, if any.
    /// `SymbolId::NONE` and every symbol the pass skipped answer `None`, which
    /// leaves the member with no `Signature` attribute.
    pub(crate) fn sig_of(&self, sym: SymbolId) -> Option<&crate::sig::GenericSignature> {
        if sym.is_none() {
            return None;
        }
        self.generic_sigs.get(&sym)
    }

    pub(crate) fn emit_anon_classes(&mut self, tree: &Tree) {
        if let TreeKind::New { tpt } = &tree.kind {
            if let TreeKind::ClassDef { name, impl_, .. } = &tpt.kind {
                if name.starts_with("$anon") {
                    self.emit_class(tpt, &HashSet::new());
                    for s in &impl_.body {
                        self.emit_anon_classes(s);
                    }
                    return;
                }
            }
        }
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.emit_anon_classes(p);
                    }
                }
                for p in &impl_.parents {
                    self.emit_anon_classes(p);
                }
                for s in &impl_.body {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for p in &impl_.parents {
                    self.emit_anon_classes(p);
                }
                for s in &impl_.body {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                self.emit_anon_classes(tpt);
                self.emit_anon_classes(rhs);
            }
            TreeKind::DefDef {
                vparamss, tpt, rhs, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.emit_anon_classes(p);
                    }
                }
                self.emit_anon_classes(tpt);
                self.emit_anon_classes(rhs);
            }
            TreeKind::Block { stats, expr } => {
                // Local `object` names declared alongside a local `case
                // class` in this same block: like `walk_stats` at the top
                // level, a user-written companion suppresses the synthetic
                // one (`type_module` already merged the two).
                let mut module_names = HashSet::new();
                for s in stats {
                    if let TreeKind::ModuleDef { name, .. } = &s.kind {
                        module_names.insert(name.clone());
                    }
                }
                for s in stats {
                    // Local `class` / `object` declared inside a method body.
                    match &s.kind {
                        TreeKind::ClassDef {
                            name, mods, impl_, ..
                        } => {
                            self.emit_class(s, &HashSet::new());
                            // A local `case class` needs its companion
                            // module class (`apply`/`unapply`) emitted too,
                            // exactly like a top-level one in `walk_stats` —
                            // otherwise `P(1)` type-checks (the typer linked
                            // a companion symbol in `ensure_companion`) but
                            // `Main$P$1$` never reaches the classfile and
                            // the call fails at run time with
                            // `NoClassDefFoundError`.
                            if mods.flags.contains(Flags::CASE) && !module_names.contains(name) {
                                self.emit_case_companion(s);
                            }
                            // ... and so do the classes and objects declared
                            // *inside* it. `walk_stats` does this for a
                            // top-level class; this walk stopped at the local
                            // one itself, so `def f = { class Outer { class
                            // Inner } }` wrote `Test$Outer$1` and nothing
                            // else, and the first `new Inner` inside it threw
                            // `NoClassDefFoundError: Test$Outer$1$Inner`.
                            self.walk_stats(&impl_.body);
                        }
                        TreeKind::ModuleDef { impl_, .. } => {
                            self.emit_module(s, &HashSet::new(), None);
                            self.walk_stats(&impl_.body);
                        }
                        _ => {}
                    }
                    self.emit_anon_classes(s);
                }
                self.emit_anon_classes(expr);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.emit_anon_classes(cond);
                self.emit_anon_classes(thenp);
                self.emit_anon_classes(elsep);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.emit_anon_classes(cond);
                self.emit_anon_classes(body);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.emit_anon_classes(lhs);
                self.emit_anon_classes(rhs);
            }
            TreeKind::Match { selector, cases } => {
                self.emit_anon_classes(selector);
                for c in cases {
                    self.emit_anon_classes(&c.pat);
                    self.emit_anon_classes(&c.guard);
                    self.emit_anon_classes(&c.body);
                }
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.emit_anon_classes(p);
                }
                self.emit_anon_classes(body);
            }
            TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
                self.emit_anon_classes(fun);
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::Typed { expr, tpt } => {
                self.emit_anon_classes(expr);
                self.emit_anon_classes(tpt);
            }
            TreeKind::Select { qual, .. } => self.emit_anon_classes(qual),
            TreeKind::Return { expr } | TreeKind::Throw { expr } => self.emit_anon_classes(expr),
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.emit_anon_classes(block);
                for c in catches {
                    self.emit_anon_classes(&c.pat);
                    self.emit_anon_classes(&c.body);
                }
                self.emit_anon_classes(finalizer);
            }
            TreeKind::InterpolatedString { args, .. } => {
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::UnApply { fun, args } => {
                self.emit_anon_classes(fun);
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::LabelDef { params, rhs, .. } => {
                for p in params {
                    self.emit_anon_classes(p);
                }
                self.emit_anon_classes(rhs);
            }
            _ => {}
        }
    }

    pub(crate) fn walk(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => self.walk_stats(stats),
            TreeKind::ClassDef { .. } => {
                self.emit_class(tree, &HashSet::new());
            }
            TreeKind::ModuleDef { .. } => {
                self.emit_module(tree, &HashSet::new(), None);
            }
            _ => {}
        }
    }

    pub(crate) fn walk_stats(&mut self, stats: &[Tree]) {
        let mut module_names = HashSet::new();
        let mut class_names = HashSet::new();
        // A value class's companion -- written or synthesized -- is where nsc
        // declares its `$extension` methods, so a module needs to know it is
        // one before it is emitted.
        let mut value_classes: HashMap<&String, &Tree> = HashMap::new();
        for s in stats {
            match &s.kind {
                TreeKind::ModuleDef { name, .. } => {
                    module_names.insert(name.clone());
                }
                TreeKind::ClassDef { name, .. } => {
                    class_names.insert(name.clone());
                    if self.st.is_value_class(s.sym) {
                        value_classes.insert(name, s);
                    }
                }
                _ => {}
            }
        }
        for s in stats {
            match &s.kind {
                TreeKind::PackageDef { .. } => self.walk(s),
                TreeKind::ClassDef {
                    name, mods, impl_, ..
                } => {
                    self.emit_class(s, &module_names);
                    if mods.flags.contains(Flags::CASE) && !module_names.contains(name) {
                        self.emit_case_companion(s);
                    }
                    // A value class with no companion of its own still needs
                    // one: that is where nsc declares (and reads) its
                    // `$extension` methods. See `emit_value_companion`. A
                    // `case class ... extends AnyVal` already gets one from
                    // `emit_case_companion`, which carries the forwarders.
                    if !module_names.contains(name)
                        && !mods.flags.contains(Flags::CASE)
                        && self.st.is_value_class(s.sym)
                    {
                        self.emit_value_companion(s);
                    }
                    self.walk_stats(&impl_.body);
                }
                TreeKind::ModuleDef { impl_, name, .. } => {
                    self.emit_module(s, &class_names, value_classes.get(name).copied());
                    self.walk_stats(&impl_.body);
                }
                _ => {}
            }
        }
    }

    /// How many lambda bodies are already queued for an *outer* classfile.
    /// A class emitted in the middle of another one (an anonymous class, a
    /// trait's own interface) must not steal its enclosing class's queue.
    pub(crate) fn lambda_watermark(&self) -> usize {
        self.lambda_bodies.borrow().len()
    }

    /// Write every lambda body queued since `base` as a static method of `b`.
    /// Bodies emitted here can themselves contain lambdas, which land on the
    /// same queue, so this runs until the queue is back down to `base`.
    pub(crate) fn drain_lambdas(&self, b: &mut ClassBuilder, base: usize) {
        loop {
            let pb = {
                let mut q = self.lambda_bodies.borrow_mut();
                if q.len() <= base {
                    break;
                }
                q.pop().expect("queue longer than watermark")
            };
            emit_lambda_body(
                b,
                self.st,
                &self.extras,
                &self.lambda_n,
                &self.lambda_bodies,
                self.source_name,
                self.library_abi,
                &self.boxed_vars,
                std::rc::Rc::clone(&self.emit_errors),
                pb,
            );
        }
    }

    pub(crate) fn emit_class(&mut self, tree: &Tree, module_names: &HashSet<String>) {
        let lambda_wm = self.lambda_watermark();
        let (name, mods, vparamss, impl_) = match &tree.kind {
            TreeKind::ClassDef {
                name,
                mods,
                vparamss,
                impl_,
                ..
            } => (name, mods, vparamss, impl_),
            _ => return,
        };
        let class_id = tree.sym;
        // A top-level `class Test` with an `object Test` beside it is where
        // that object's static forwarders go, so this classfile cannot be
        // written until the object has been emitted. See
        // [`Gen::finish_companion_class`]; the `object` may come either side
        // of the class in the file.
        let has_object = module_names.contains(name)
            && !class_id.is_none()
            && matches!(
                self.st.get(self.st.get(class_id).owner).kind,
                SymKind::Package | SymKind::NoSymbol
            );
        let this_name = if class_id.is_none() {
            name.clone()
        } else {
            class_internal(self.st, class_id)
        };
        let is_trait = mods.flags.contains(Flags::TRAIT);
        let (super_name, interfaces) = split_parents(self.st, &impl_.parents);

        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.super_name = super_name;
        b.interfaces = interfaces;
        if !is_trait && mods.flags.contains(Flags::CASE) {
            self.add_product_interfaces(&mut b);
        }

        if is_trait {
            b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
            b.super_name = "java/lang/Object".into();
            for stt in &impl_.body {
                if let TreeKind::DefDef {
                    name, mods, rhs, ..
                } = &stt.kind
                {
                    if name == "<init>" || name == "<clinit>" {
                        continue;
                    }
                    // A genuinely private trait method (JVMS 4.6 forbids
                    // `ACC_PRIVATE | ACC_ABSTRACT`) never appears as a
                    // declaration: every caller lives inside the trait's own
                    // code, so nothing outside needs an abstract signature to
                    // dispatch through. See `is_trait_private_def`.
                    //
                    // A *concrete* method gets a `default` method with the
                    // body from `emit_trait_bodies` instead of an abstract
                    // declaration here.
                    if !is_trait_private_def(self.st, stt) && rhs.is_empty() {
                        let acc = method_access_flags(mods.flags, widened(self.st, stt.sym))
                            | ACC_ABSTRACT;
                        b.add_abstract(acc, name, &def_method_desc(self.st, stt));
                        b.sign_last(self.sig_of(stt.sym));
                    }
                    let mut super_accesses = Vec::new();
                    collect_super_accesses(rhs, &mut super_accesses);
                    for (super_name, super_sym) in super_accesses {
                        let acc_name = super_accessor_name(self.st, class_id, &super_name);
                        let desc = if super_name == name.as_str() {
                            def_method_desc(self.st, stt)
                        } else if !super_sym.is_none() {
                            method_desc_from_sym(self.st, super_sym)
                        } else {
                            def_method_desc(self.st, stt)
                        };
                        if b.methods.iter().any(|m| m.name == acc_name) {
                            continue;
                        }
                        b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, &acc_name, &desc);
                    }
                }
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &stt.kind
                {
                    let ty = val_tree_ty(self.st, stt);
                    let gdesc = format!("(){}", jvm_desc(self.st, &ty));
                    // A `lazy val` with an initialiser is the one `val` whose
                    // accessor is *concrete* on the interface: nsc puts the
                    // initialiser in a `default` method with the usual `m$`
                    // static beside it and leaves the caching to the
                    // implementing class. `emit_trait_bodies` emits both.
                    if !(mods.flags.contains(Flags::LAZY) && !rhs.is_empty()) {
                        b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, name, &gdesc);
                        b.sign_last_accessor(self.sig_of(stt.sym), false);
                    }
                    let sdesc = format!("({})V", jvm_desc_val(self.st, &ty));
                    if mods.flags.contains(Flags::MUTABLE) {
                        // A trait `var` — abstract or not — is a getter plus a
                        // public `v_$eq`, exactly as nsc emits it.
                        b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, &var_setter_name(name), &sdesc);
                        b.sign_last_accessor(self.sig_of(stt.sym), true);
                    } else if !rhs.is_empty() && !mods.flags.contains(Flags::LAZY) {
                        b.add_abstract(
                            ACC_PUBLIC | ACC_ABSTRACT,
                            &trait_val_setter_name(self.st, class_id, name),
                            &sdesc,
                        );
                        b.sign_last_accessor(self.sig_of(stt.sym), true);
                    }
                }
            }
            // A trait nested in a class reaches the enclosing instance
            // through an accessor the interface declares and every
            // implementing class fills in (`emit_trait_outer_accessors`).
            if let Some(o) = outer_field_class(self.st, class_id) {
                b.add_abstract(
                    ACC_PUBLIC | ACC_ABSTRACT,
                    &trait_outer_accessor_name(self.st, class_id),
                    &format!("()L{};", class_internal(self.st, o)),
                );
            }
            // A member `object` of a trait is one instance per implementing
            // instance: the interface only declares the accessor, and every
            // class that mixes the trait in gets the field and the body.
            for mcls in self.member_modules_of(class_id, &impl_.body) {
                b.add_abstract(
                    ACC_PUBLIC | ACC_ABSTRACT,
                    &module_accessor_name(self.st, mcls),
                    &module_accessor_desc(self.st, mcls),
                );
            }
            // A local trait reads what it captured the same way: the
            // interface declares an accessor per captured symbol and the
            // implementing class fills it in from its own capture field.
            for (_, aname, adesc, _) in trait_capture_accessors(self.st, &self.boxed_vars, class_id)
            {
                b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, &aname, &adesc);
            }
            // `def m(a: A, b: B = …)` in a trait: the `m$default$2` getter is
            // a synthesized *symbol*, not a tree, so the loop above never saw
            // it. Declare it here; `emit_default_getters` puts the body on
            // every implementing class.
            for (n, d) in self.default_getter_sigs(class_id) {
                b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT | ACC_SYNTHETIC, &n, &d);
            }
            // The concrete half: `default` methods, their `m$` statics and
            // `$init$`, all on the interface itself (nsc 2.13's trait ABI).
            self.emit_trait_bodies(&mut b, class_id, &this_name);
            attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
            b.sign_class(self.sig_of(class_id));
            self.finish_companion_class(b, has_object);
            // `emit_trait_bodies` drains whatever the trait's own bodies
            // queued, onto the interface (nsc puts `$anonfun$` statics there
            // too). Nothing may leak into the enclosing classfile's queue.
            debug_assert_eq!(
                self.lambda_watermark(),
                lambda_wm,
                "a lambda body was queued while emitting the interface {this_name}"
            );
            return;
        }

        b.access = ACC_PUBLIC | ACC_SUPER;
        if mods.flags.contains(Flags::FINAL) {
            b.access |= ACC_FINAL;
        }

        // constructor / body fields
        for clause in vparamss {
            for p in clause {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                        self.st.get(p.sym).ty.clone()
                    } else {
                        p.ty.clone()
                    };
                    b.fields.push(Field {
                        access: field_access_flags(mods.flags, widened(self.st, p.sym)),
                        name: name.clone(),
                        desc: jvm_desc_val(self.st, &ty),
                    });
                    b.sign_field(name, self.sig_of(p.sym));
                }
            }
        }
        if let Some(outer_desc) = outer_field_desc(self.st, class_id) {
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: "$outer".into(),
                desc: outer_desc,
            });
        }
        // Enclosing-method locals read by the body of a class defined inside a
        // method. Public so lambdas lifted out of this class can read them.
        for (_, fname, fdesc, _) in capture_slots(self.st, &self.boxed_vars, class_id) {
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: fname,
                desc: fdesc,
            });
        }
        for stt in &impl_.body {
            if let TreeKind::ValDef { name, mods, .. } = &stt.kind {
                let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                    self.st.get(stt.sym).ty.clone()
                } else {
                    stt.ty.clone()
                };
                b.fields.push(Field {
                    access: field_access_flags(mods.flags, widened(self.st, stt.sym)),
                    name: name.clone(),
                    desc: jvm_desc_val(self.st, &ty),
                });
                b.sign_field(name, self.sig_of(stt.sym));
            }
        }
        // `@SerialVersionUID(n)`: nsc emits a `private static final long
        // serialVersionUID` whose value lives in a JVMS §4.7.2 `ConstantValue`
        // -- there is no `<clinit>` store, and `ObjectStreamClass.lookup`
        // reads the attribute. Without the field the JVM computes a UID from
        // the class's shape instead (`run/t6988`).
        if let Some(uid) = serial_version_uid(mods) {
            if !b.fields.iter().any(|f| f.name == "serialVersionUID") {
                b.fields.push(Field {
                    access: ACC_PRIVATE | ACC_STATIC | ACC_FINAL,
                    name: "serialVersionUID".into(),
                    desc: "J".into(),
                });
                b.field_constants.insert("serialVersionUID".into(), uid);
            }
        }
        for (name, ty, extra) in self.mixin_val_fields(class_id, vparamss, &impl_.body) {
            b.fields.push(Field {
                access: ACC_PUBLIC | extra,
                name,
                desc: jvm_desc_val(self.st, &ty),
            });
        }
        let lazies = self.all_lazy_vals(class_id, &impl_.body);
        let binary_lazies = self.binary_mixin_lazy_vals(class_id, &impl_.body);
        for v in &self.mixin_lazy_vals(class_id, &impl_.body) {
            b.fields.push(Field {
                access: ACC_PRIVATE,
                name: v.name().unwrap_or("").to_string(),
                desc: jvm_desc_val(self.st, &val_tree_ty(self.st, v)),
            });
        }
        for v in &binary_lazies {
            b.fields.push(Field {
                access: ACC_PRIVATE,
                name: v.name.clone(),
                desc: jvm_desc_val(self.st, &v.ty),
            });
        }
        if !lazies.is_empty() || !binary_lazies.is_empty() {
            b.fields
                .extend(self.lazy_bitmap_fields(&lazies, binary_lazies.len()));
        }
        self.emit_class_ctor(&mut b, class_id, vparamss, &impl_.body, &impl_.parents);
        let own_modules = self.member_modules_of(class_id, &impl_.body);
        let mixin_modules = self.mixin_member_modules(class_id, &own_modules);
        self.emit_member_module_accessors(&mut b, &own_modules);
        self.emit_member_module_accessors(&mut b, &mixin_modules);
        self.emit_trait_outer_accessors(&mut b, class_id);
        self.emit_lazy_accessors(&mut b, class_id, &lazies, &binary_lazies);
        self.emit_val_getters(&mut b, &impl_.body);
        self.emit_ctor_val_getters(&mut b, class_id, vparamss);
        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, class_id, stt);
                if self.st.is_value_class(class_id) {
                    self.emit_value_extension(&mut b, class_id, stt);
                }
            }
        }
        if self.st.get(class_id).flags.contains(Flags::CASE)
            && !impl_.body.iter().any(|t| t.name() == Some("copy"))
        {
            emit_case_copy(&mut b, self.st, class_id);
        }
        self.emit_default_getters(&mut b, class_id);
        self.emit_trait_val_accessors(&mut b, class_id, &impl_.body);
        self.emit_super_accessors(&mut b, class_id);
        self.emit_mixin_forwarders(&mut b, class_id, &impl_.body);
        self.emit_delayed_init_support(&mut b, class_id, &impl_.body, false);
        self.emit_case_object_methods(&mut b, class_id);
        self.emit_value_class_methods(&mut b, class_id);
        self.emit_erasure_bridges(&mut b, class_id);
        self.emit_inherited_covariant_bridges(&mut b, class_id);
        self.emit_binary_parent_bridges(&mut b, class_id);
        self.drain_lambdas(&mut b, lambda_wm);
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        b.sign_class(self.sig_of(class_id));
        self.finish_companion_class(b, has_object);
    }

    pub(crate) fn delayed_body_class(class_name: &str) -> String {
        format!("{}$delayedInit$body", class_name.replace('/', "$"))
    }

    pub(crate) fn emit_delayed_init_support(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
        is_module: bool,
    ) {
        if class_id.is_none() || !extends_delayed_init(self.st, class_id) {
            return;
        }
        let class_name = b.this_name.clone();
        let is_app = extends_app(self.st, class_id);
        if is_app {
            if self.library_abi {
                self.emit_app_library_members(b, &class_name, is_module);
            } else {
                self.emit_app_private_members(b, &class_name);
            }
        }
        self.emit_delayed_endpoint(b, class_id, body);
        self.emit_delayed_init_lambda(&class_name);
    }

    pub(crate) fn emit_app_private_members(&self, b: &mut ClassBuilder, class_name: &str) {
        let already = b.methods.iter().any(|m| m.name == "delayedInit");
        b.fields.push(Field {
            access: ACC_PRIVATE,
            name: "scala$App$$delayed".into(),
            desc: "Lscala/Function0;".into(),
        });
        if !already {
            let cn = class_name.to_string();
            b.add_code(
                ACC_PUBLIC,
                "delayedInit",
                "(Lscala/Function0;)V",
                2,
                |asm| {
                    asm.aload(0);
                    asm.aload(1);
                    asm.putfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                    asm.vreturn();
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "main") {
            let cn = class_name.to_string();
            b.add_code(ACC_PUBLIC, "main", "([Ljava/lang/String;)V", 2, |asm| {
                asm.aload(0);
                asm.getfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                let done = asm.fresh_label();
                asm.ifnull(done);
                asm.aload(0);
                asm.getfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
                asm.pop();
                asm.mark(done);
                asm.vreturn();
            });
        }
    }

    pub(crate) fn emit_app_library_members(
        &self,
        b: &mut ClassBuilder,
        class_name: &str,
        is_module: bool,
    ) {
        let acc_f = if is_module {
            ACC_PRIVATE | ACC_STATIC
        } else {
            ACC_PRIVATE
        };
        b.fields.push(Field {
            access: acc_f,
            name: "executionStart".into(),
            desc: "J".into(),
        });
        b.fields.push(Field {
            access: acc_f,
            name: "scala$App$$_args".into(),
            desc: "[Ljava/lang/String;".into(),
        });
        b.fields.push(Field {
            access: acc_f,
            name: "scala$App$$initCode".into(),
            desc: "Lscala/collection/mutable/ListBuffer;".into(),
        });
        let cn = class_name.to_string();
        if is_module {
            b.add_code(ACC_PUBLIC, "executionStart", "()J", 1, {
                let cn = cn.clone();
                move |asm| {
                    asm.getstatic(&cn, "executionStart", "J");
                    asm.lreturn();
                }
            });
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$executionStart_$eq",
                "(J)V",
                3,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.lload(1);
                        asm.putstatic(&cn, "executionStart", "J");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args",
                "()[Ljava/lang/String;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.getstatic(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args_$eq",
                "([Ljava/lang/String;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(1);
                        asm.putstatic(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$initCode",
                "()Lscala/collection/mutable/ListBuffer;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.getstatic(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$scala$App$$initCode_$eq",
                "(Lscala/collection/mutable/ListBuffer;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(1);
                        asm.putstatic(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.vreturn();
                    }
                },
            );
        } else {
            b.add_code(ACC_PUBLIC, "executionStart", "()J", 1, {
                let cn = cn.clone();
                move |asm| {
                    asm.aload(0);
                    asm.getfield(&cn, "executionStart", "J");
                    asm.lreturn();
                }
            });
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$executionStart_$eq",
                "(J)V",
                3,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.lload(1);
                        asm.putfield(&cn, "executionStart", "J");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args",
                "()[Ljava/lang/String;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.getfield(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args_$eq",
                "([Ljava/lang/String;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.aload(1);
                        asm.putfield(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$initCode",
                "()Lscala/collection/mutable/ListBuffer;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.getfield(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$scala$App$$initCode_$eq",
                "(Lscala/collection/mutable/ListBuffer;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.aload(1);
                        asm.putfield(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.vreturn();
                    }
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "delayedInit") {
            b.add_code(
                ACC_PUBLIC,
                "delayedInit",
                "(Lscala/Function0;)V",
                2,
                |asm| {
                    asm.aload(0);
                    asm.aload(1);
                    asm.invokestatic_interface(
                        "scala/App",
                        "delayedInit$",
                        "(Lscala/App;Lscala/Function0;)V",
                    );
                    asm.vreturn();
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "main") {
            b.add_code(ACC_PUBLIC, "main", "([Ljava/lang/String;)V", 2, |asm| {
                asm.aload(0);
                asm.aload(1);
                asm.invokestatic_interface(
                    "scala/App",
                    "main$",
                    "(Lscala/App;[Ljava/lang/String;)V",
                );
                asm.vreturn();
            });
        }
        if !b.methods.iter().any(|m| m.name == "args") {
            b.add_code(ACC_PUBLIC, "args", "()[Ljava/lang/String;", 1, |asm| {
                asm.aload(0);
                asm.invokestatic_interface(
                    "scala/App",
                    "args$",
                    "(Lscala/App;)[Ljava/lang/String;",
                );
                asm.areturn();
            });
        }
    }

    pub(crate) fn emit_delayed_endpoint(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
    ) {
        let class_name = b.this_name.clone();
        let st = self.st;
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let stats: Vec<Tree> = body
            .iter()
            .filter(|t| is_delayed_ctor_stat(t) && !is_presuper_val(t))
            .cloned()
            .collect();
        b.add_code(ACC_PUBLIC, "delayedEndpoint$body", "()V", 4, |asm| {
            let mut frame = Frame::instance();
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
                std::rc::Rc::clone(&self.emit_errors),
            );
            for stt in &stats {
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &stt.kind
                {
                    if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx, rhs);
                    let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                        st.get(stt.sym).ty.clone()
                    } else {
                        stt.ty.clone()
                    };
                    emit_putfield_from_expr(asm, &class_name, name, &jvm_desc_val(st, &ty));
                } else {
                    gen_expr(asm, &mut frame, &ctx, stt);
                    pop_if_value(asm, &stt.ty);
                }
            }
            asm.vreturn();
        });
    }

    pub(crate) fn emit_delayed_init_lambda(&self, class_name: &str) {
        let lam = Self::delayed_body_class(class_name);
        let mut b = ClassBuilder::new(lam.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_SUPER | ACC_SYNTHETIC | ACC_FINAL;
        b.interfaces.push("scala/Function0".into());
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: "$outer".into(),
            desc: format!("L{class_name};"),
        });
        let outer_d = format!("L{class_name};");
        let lam_c = lam.clone();
        let cn = class_name.to_string();
        b.add_code(ACC_PUBLIC, "<init>", &format!("({outer_d})V"), 2, |asm| {
            asm.aload(0);
            asm.invokespecial("java/lang/Object", "<init>", "()V");
            asm.aload(0);
            asm.aload(1);
            asm.putfield(&lam_c, "$outer", &outer_d);
            asm.vreturn();
        });
        b.add_code(ACC_PUBLIC, "apply", "()Ljava/lang/Object;", 1, |asm| {
            asm.aload(0);
            asm.getfield(&lam, "$outer", &format!("L{cn};"));
            asm.invokevirtual(&cn, "delayedEndpoint$body", "()V");
            asm.aconst_null();
            asm.areturn();
        });
        self.extras.borrow_mut().push(b.finish());
    }

    pub(crate) fn emit_delayed_init_call(asm: &mut crate::code::Assembler, class_name: &str) {
        let lam = Self::delayed_body_class(class_name);
        asm.aload(0);
        asm.new_obj(&lam);
        asm.dup();
        asm.aload(0);
        asm.invokespecial(&lam, "<init>", &format!("(L{class_name};)V"));
        asm.invokevirtual(class_name, "delayedInit", "(Lscala/Function0;)V");
    }

    pub(crate) fn emit_class_ctor(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
        parents: &[Tree],
    ) {
        let params: Vec<&Tree> = vparamss.iter().flatten().collect();
        let mut frame = Frame::instance();
        let outer = outer_field_class(self.st, class_id);
        let outer_desc = outer_field_desc(self.st, class_id);
        if outer.is_some() {
            frame.next_slot += 1; // slot 1 is $outer
        }
        let mut param_info = Vec::new();
        for p in &params {
            let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                self.st.get(p.sym).ty.clone()
            } else {
                p.ty.clone()
            };
            // The field store below moves the argument straight through, so
            // the slot sort is what the JVM actually passes: a `Unit`
            // parameter arrives as `BoxedUnit`, not as nothing.
            let sort = jvm_slot_sort(&ty);
            let slot = frame.alloc_param(p.sym, jvm_sort(&ty), &ty);
            let fname = p.name().unwrap_or("").to_string();
            param_info.push((slot, sort, fname, jvm_desc_val(self.st, &ty)));
        }
        // The class body reaches a `var` parameter through its *field*, the
        // way nsc does. While the constructor's own local stayed bound to the
        // symbol, `class C(private[this] var c: String) { c = "good" }` stored
        // to the local and left the field holding the constructor argument,
        // so `def f = c` still answered `"bad"`. Only `var` parameters need
        // this: an immutable one can never disagree with its field.
        let mutable_params: Vec<SymbolId> = params
            .iter()
            .map(|p| p.sym)
            .filter(|s| !s.is_none() && self.st.get(*s).flags.contains(Flags::MUTABLE))
            .collect();
        let mut types: Vec<Type> = Vec::new();
        if let Some(o) = outer {
            types.push(Type::Class {
                sym: o,
                args: vec![],
            });
        }
        for p in &params {
            if p.ty.is_no_type() && !p.sym.is_none() {
                types.push(self.st.get(p.sym).ty.clone());
            } else {
                types.push(p.ty.clone());
            }
        }
        // Captures come last, after `$outer` and the source parameters.
        let caps = capture_slots(self.st, &self.boxed_vars, class_id);
        let mut cap_info = Vec::new();
        for (id, fname, fdesc, sort) in &caps {
            let slot = frame.alloc(*id, *sort);
            cap_info.push((slot, *sort, fname.clone(), fdesc.clone()));
        }
        let desc = desc_with_extra_params(
            &jvm_method_desc(self.st, &types, &Type::Unit),
            &capture_params_desc(self.st, &self.boxed_vars, class_id),
        );
        let super_name = b.super_name.clone();
        let (super_owner, super_desc, super_args, super_cls, super_field_tys) =
            parent_super_ctor(self.st, parents, &super_name);
        let super_outer = outer_field_class(self.st, super_cls);
        let class_name = b.this_name.clone();
        let st = self.st;
        // `val` initializers *and* the body's bare statements, in source order.
        let inits: Vec<&Tree> = template_init_stats(body);
        let max_locals = frame.next_slot.max(4);
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let delayed = extends_delayed_init(st, class_id);
        let is_app = extends_app(st, class_id);
        let has_outer = outer.is_some();
        let outer_desc_c = outer_desc.clone();
        let mixin_inits = self.mixin_init_calls(class_id);
        b.add_code(ACC_PUBLIC, "<init>", &desc, max_locals, |asm| {
            let mut frame = frame;
            let mut ctx_early = emit_ctx(
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
                std::rc::Rc::clone(&self.emit_errors),
            );
            // nsc stores `$outer` *before* the super constructor call, so a
            // method the parent's `<init>` dispatches back to this class
            // already sees the enclosing instance. JVMS §4.10.1.9 allows a
            // `putfield` of a field declared in the current class on
            // `uninitializedThis` -- but never a `getfield`, which is why the
            // pre-super code below reads the argument instead of the field.
            if has_outer {
                ctx_early.presuper_outer = presuper_outer_of(st, class_id);
                if let Some(od) = &outer_desc_c {
                    asm.aload(0);
                    asm.aload(1);
                    asm.putfield(&class_name, "$outer", od);
                }
            }
            // nsc: early vals are stored to fields before the superclass ctor so
            // parent / trait `$init$` bodies see the values.
            for vd in &inits {
                if !is_presuper_val(vd) {
                    continue;
                }
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &vd.kind
                {
                    if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx_early, rhs);
                    let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                        st.get(vd.sym).ty.clone()
                    } else {
                        vd.ty.clone()
                    };
                    emit_putfield_from_expr(asm, &class_name, name, &jvm_desc_val(st, &ty));
                }
            }
            asm.aload(0);
            // A nested superclass takes its enclosing instance first. Our own
            // `$outer` is not stored yet, so read it out of the argument.
            if let Some(o) = super_outer {
                if has_outer && is_owner_compatible(st, outer.unwrap_or(SymbolId::NONE), o) {
                    asm.aload(1);
                } else {
                    load_outer_arg(asm, &ctx_early, o);
                }
            }
            for (i, a) in super_args.iter().enumerate() {
                gen_expr(asm, &mut frame, &ctx_early, a);
                // `class D extends B((), 5)`: the super constructor takes a
                // `BoxedUnit` there and the `()` left nothing on the stack.
                // Erasure has already `$box`ed any `()` that goes to an
                // `Object` parameter, so a `Unit`-typed argument here really
                // does mean a `Unit` parameter.
                adapt_unit_arg(asm, &ctx_early, a, &a.ty);
                // `class A1 extends AtomicReference[Int](1)`: a generic
                // (often Java) superclass ctor takes `Object`, but a
                // primitive argument is still on the stack unboxed. Same
                // check `gen_new` makes for a plain `new`.
                let pty = super_field_tys.get(i).unwrap_or(&a.ty);
                if is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty) {
                    emit_box(asm, &a.ty);
                }
            }
            asm.invokespecial(&super_owner, "<init>", &super_desc);
            for (slot, sort, fname, fdesc) in &param_info {
                if fname.is_empty() {
                    continue;
                }
                asm.aload(0);
                load(asm, *slot, *sort);
                asm.putfield(&class_name, fname, fdesc);
            }
            for (slot, sort, fname, fdesc) in &cap_info {
                asm.aload(0);
                load(asm, *slot, *sort);
                asm.putfield(&class_name, fname, fdesc);
            }
            // From here on the field is the parameter: unbind the local so the
            // body's reads and writes go through it (see `mutable_params`).
            for id in &mutable_params {
                frame.locals.remove(id);
            }
            for (iface, init_desc) in &mixin_inits {
                asm.aload(0);
                asm.invokestatic_interface(iface, "$init$", init_desc);
            }
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
                std::rc::Rc::clone(&self.emit_errors),
            );
            if delayed {
                if library_abi && is_app {
                    asm.aload(0);
                    asm.invokestatic_interface("scala/App", "$init$", "(Lscala/App;)V");
                }
                Gen::emit_delayed_init_call(asm, &class_name);
            } else {
                for vd in &inits {
                    if is_presuper_val(vd) {
                        continue;
                    }
                    if let TreeKind::ValDef {
                        name, mods, rhs, ..
                    } = &vd.kind
                    {
                        if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                            continue;
                        }
                        asm.aload(0);
                        gen_expr(asm, &mut frame, &ctx, rhs);
                        let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                            st.get(vd.sym).ty.clone()
                        } else {
                            vd.ty.clone()
                        };
                        emit_putfield_from_expr(asm, &class_name, name, &jvm_desc_val(st, &ty));
                    } else {
                        // A bare statement of the template body (SLS 5.1),
                        // in its source position among the `val` stores.
                        gen_stat(asm, &mut frame, &ctx, vd);
                    }
                }
            }
            asm.vreturn();
        });
    }

    pub(crate) fn emit_def(&self, b: &mut ClassBuilder, class_id: SymbolId, def: &Tree) {
        let (name, mods, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                mods,
                vparamss,
                rhs,
                ..
            } => (name, mods, vparamss, rhs),
            _ => return,
        };
        if name == "<clinit>" {
            return;
        }
        if name == "<init>" && rhs.is_empty() {
            return;
        }
        let mut desc = def_method_desc_boxed(self.st, def, &self.boxed_vars);
        // An *auxiliary* constructor of an inner class takes the enclosing
        // instance too, exactly as the primary one does (`emit_class_ctor`).
        // Without it slick's `abstract class Table[T](tag, schema, name)`
        // emitted its `def this(tag, name)` as `(Tag, String)V` while nsc --
        // and therefore any client compiled against nsc's classfiles -- calls
        // `(RelationalProfile, Tag, String)V`: `NoSuchMethodError` on the
        // first table definition.
        let ctor_outer = (name == "<init>")
            .then(|| outer_field_class(self.st, class_id))
            .flatten();
        if ctor_outer.is_some() {
            desc = with_enclosing_outer_param(self.st, class_id, &desc);
        }
        let ret = method_ret_ty(def);
        let acc = method_access_flags(mods.flags, widened(self.st, def.sym));
        if mods.flags.contains(Flags::NATIVE) {
            b.add_abstract(acc, name, &desc);
            b.sign_last(self.sig_of(def.sym));
            if let Some(d) = java_deprecated_desc(mods) {
                b.add_java_annot_to_last(d);
            }
            return;
        }
        if rhs.is_empty() {
            b.add_abstract(acc | ACC_ABSTRACT, name, &desc);
            b.sign_last(self.sig_of(def.sym));
            if let Some(d) = java_deprecated_desc(mods) {
                b.add_java_annot_to_last(d);
            }
            return;
        }
        let mut frame = Frame::instance();
        if ctor_outer.is_some() {
            frame.next_slot += 1; // slot 1 is $outer
        }
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                let sort = if def_is_synthetic(self.st, def)
                    && !p.sym.is_none()
                    && self.boxed_vars.contains(&p.sym)
                {
                    JvmSort::Ref
                } else {
                    jvm_sort(&ty)
                };
                frame.alloc_param(p.sym, sort, &ty);
            }
        }
        let class_name = b.this_name.clone();
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
        let caps = if acc & ACC_STATIC == 0 {
            capture_slots(self.st, &self.boxed_vars, class_id)
        } else {
            CaptureSlots::new()
        };
        let mut tailrec_error = None;
        b.add_code(acc, name, &desc, max_locals, |asm| {
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
                std::rc::Rc::clone(&self.emit_errors),
            );
            ctx.method_sym = meth;
            tailrec_error = crate::gen_tailrec::begin_tail_loop(asm, &mut frame, &ctx, rhs);
            emit_capture_prologue(asm, &mut frame, &class_name, &caps);
            finish_method_body(asm, &mut frame, &ctx, rhs, &ret_for_body);
            tailrec_error = tailrec_error
                .take()
                .or_else(|| crate::gen_tailrec::finish_tail_loop(&frame, &ctx));
        });
        if let Some(error) = tailrec_error {
            b.format_errors.push(error);
        }
        b.sign_last(self.sig_of(def.sym));
        if let Some(d) = java_deprecated_desc(mods) {
            b.add_java_annot_to_last(d);
        }
        self.emit_java_varargs_forwarder(b, def, name, &desc, acc);
    }

    /// `@scala.annotation.varargs` asks for a second, Java-shaped entry point:
    /// `def f(xs: String*)` erases to `f(Seq)`, and the annotation adds
    /// `f(String[])` which wraps the array and calls it. Without it the method
    /// is simply not callable from Java, and `getDeclaredMethods` shows one
    /// overload where scalac shows two (`run/t5125`, `run/t5125b`).
    pub(crate) fn emit_java_varargs_forwarder(
        &self,
        b: &mut ClassBuilder,
        def: &Tree,
        name: &str,
        desc: &str,
        acc: u16,
    ) {
        let TreeKind::DefDef { mods, vparamss, .. } = &def.kind else {
            return;
        };
        if name == "<init>" || name == "<clinit>" {
            return;
        }
        if !mods.annotations.iter().any(|a| {
            matches!(
                a.annotation_path().as_str(),
                "varargs" | "annotation.varargs" | "scala.annotation.varargs"
            )
        }) {
            return;
        }
        // The repeated parameter is the last one of the last clause.
        let Some(last) = vparamss.last().and_then(|c| c.last()) else {
            return;
        };
        let pty = if last.ty.is_no_type() && !last.sym.is_none() {
            self.st.get(last.sym).ty.clone()
        } else {
            last.ty.clone()
        };
        let Type::Repeated(elem) = pty else { return };
        let elem_desc = jvm_desc_val(self.st, &elem);
        let array_desc = format!("[{elem_desc}");
        // `(…Lscala/collection/immutable/Seq;)R` -> `(…[T)R`.
        let Some(close) = desc.find(')') else { return };
        let inner = &desc[1..close];
        let Some(cut) = split_desc_types(inner).last().copied() else {
            return;
        };
        let head: String = inner[..cut].to_string();
        let ret_desc = desc[close + 1..].to_string();
        let fwd_desc = format!("({head}{array_desc}){ret_desc}");
        if b.methods
            .iter()
            .any(|m| m.name == name && m.desc == fwd_desc)
        {
            return;
        }
        let (wrap, wrap_desc) = match elem_desc.as_str() {
            "I" => ("wrapIntArray", "([I)Lscala/collection/immutable/ArraySeq;"),
            "J" => ("wrapLongArray", "([J)Lscala/collection/immutable/ArraySeq;"),
            "D" => (
                "wrapDoubleArray",
                "([D)Lscala/collection/immutable/ArraySeq;",
            ),
            "F" => (
                "wrapFloatArray",
                "([F)Lscala/collection/immutable/ArraySeq;",
            ),
            "S" => (
                "wrapShortArray",
                "([S)Lscala/collection/immutable/ArraySeq;",
            ),
            "B" => ("wrapByteArray", "([B)Lscala/collection/immutable/ArraySeq;"),
            "C" => ("wrapCharArray", "([C)Lscala/collection/immutable/ArraySeq;"),
            "Z" => (
                "wrapBooleanArray",
                "([Z)Lscala/collection/immutable/ArraySeq;",
            ),
            _ => (
                "wrapRefArray",
                "([Ljava/lang/Object;)Lscala/collection/immutable/ArraySeq;",
            ),
        };
        let is_static = acc & ACC_STATIC != 0;
        let head_sorts = desc_param_sorts(&format!("({head})V"));
        let mut slot: u16 = if is_static { 0 } else { 1 };
        let loads: Vec<(u16, JvmSort)> = head_sorts
            .iter()
            .map(|s| {
                let at = slot;
                slot += s.slots();
                (at, *s)
            })
            .collect();
        let array_slot = slot;
        let max_locals = slot + 1;
        let class_name = b.this_name.clone();
        let target_desc = desc.to_string();
        let mname = name.to_string();
        let private = acc & ACC_PRIVATE != 0;
        let ret = method_ret_ty(def);
        b.add_code(
            acc | ACC_VARARGS,
            name,
            &fwd_desc,
            max_locals + 1,
            move |asm| {
                if !is_static {
                    asm.aload(0);
                }
                for (at, sort) in &loads {
                    load(asm, *at, *sort);
                }
                asm.getstatic(
                    "scala/runtime/ScalaRunTime$",
                    "MODULE$",
                    "Lscala/runtime/ScalaRunTime$;",
                );
                asm.aload(array_slot);
                if wrap == "wrapRefArray" && array_desc != "[Ljava/lang/Object;" {
                    asm.checkcast("[Ljava/lang/Object;");
                }
                asm.invokevirtual("scala/runtime/ScalaRunTime$", wrap, wrap_desc);
                if is_static {
                    asm.invokestatic(&class_name, &mname, &target_desc);
                } else if private {
                    asm.invokespecial(&class_name, &mname, &target_desc);
                } else {
                    asm.invokevirtual(&class_name, &mname, &target_desc);
                }
                emit_return(asm, &ret);
            },
        );
    }

    pub(crate) fn emit_value_extension(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
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
        if rhs.is_empty() || name == "<init>" || name == "<clinit>" {
            return;
        }
        let Some(under) = self.st.value_class_underlying(class_id) else {
            return;
        };
        let field = self.st.get(class_id).ctor_fields.first().copied();
        let ext_name = format!("{name}$extension");
        let desc = value_extension_desc(self.st, def.sym);
        let ret = method_ret_ty(def);
        let mut frame = Frame {
            locals: HashMap::new(),
            next_slot: 0,
            finally_exits: Vec::new(),
            return_slot: None,
            tail_loop: None,
        };
        if let Some(fid) = field {
            frame.alloc(fid, jvm_sort(&under));
        } else {
            frame.next_slot = 1;
        }
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
        let class_name = b.this_name.clone();
        let st = self.st;
        let max_locals = frame.next_slot.max(1);
        let ret_for_body = ret.clone();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let method = def.sym;
        let mut tailrec_error = None;
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            &ext_name,
            &desc,
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
                    std::rc::Rc::clone(&self.emit_errors),
                );
                ctx.method_sym = method;
                ctx.value_ext = Some((
                    class_name.clone(),
                    format!("({})V", jvm_desc_val(st, &under)),
                    jvm_sort(&under),
                ));
                tailrec_error = crate::gen_tailrec::begin_tail_loop(asm, &mut frame, &ctx, rhs);
                gen_expr(asm, &mut frame, &ctx, rhs);
                tailrec_error = tailrec_error
                    .take()
                    .or_else(|| crate::gen_tailrec::finish_tail_loop(&frame, &ctx));
                if is_unit_like(&ret_for_body) {
                    pop_if_value(asm, &rhs.ty);
                    asm.vreturn();
                } else {
                    emit_return(asm, &ret_for_body);
                }
            },
        );
        if let Some(error) = tailrec_error {
            b.format_errors.push(error);
        }
    }

    /// nsc's `SyntheticMethods` for a value class. A boxed `Meters(5)` has to
    /// compare and hash by its underlying value -- otherwise `m == new
    /// Meters(5)` is reference equality once either side is boxed, and
    /// `Object.toString` prints an identity hash where nsc prints `Meters@5`.
    /// The `$extension` statics mirror what nsc puts on the class.
    pub(crate) fn emit_value_class_methods(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if !self.st.is_value_class(class_id) {
            return;
        }
        let Some(&f) = self.st.get(class_id).ctor_fields.first() else {
            return;
        };
        let fname = self.st.get(f).name.clone();
        let fty = self.st.get(f).ty.clone();
        let fdesc = jvm_desc(self.st, &fty);
        let cj = b.this_name.clone();
        let defined: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        let wide = matches!(fty, Type::Long | Type::Double);
        let fslots = if wide { 2u16 } else { 1 };

        // `hashCode$extension(u)` = the underlying's own hash, which is what
        // `Integer.hashCode(n)` gives for the common `Int` case.
        if !defined.contains("hashCode$extension") {
            let (t, d) = (fty.clone(), fdesc.clone());
            b.add_code(
                ACC_PUBLIC | ACC_STATIC,
                "hashCode$extension",
                &format!("({d})I"),
                fslots,
                move |asm| {
                    load(asm, 0, jvm_sort(&t));
                    if is_jvm_primitive(&t) {
                        emit_box(asm, &t);
                    }
                    asm.invokestatic("java/util/Objects", "hashCode", "(Ljava/lang/Object;)I");
                    asm.ireturn();
                },
            );
        }
        if !defined.contains("hashCode") {
            let (cj2, fname2, fdesc2, d) =
                (cj.clone(), fname.clone(), fdesc.clone(), fdesc.clone());
            let cj3 = cj.clone();
            b.add_code(ACC_PUBLIC, "hashCode", "()I", 1, move |asm| {
                asm.aload(0);
                asm.getfield(&cj2, &fname2, &fdesc2);
                asm.invokestatic(&cj3, "hashCode$extension", &format!("({d})I"));
                asm.ireturn();
            });
        }

        // `equals$extension(u, that)` = `that.isInstanceOf[C] && u == that.u`.
        if !defined.contains("equals$extension") {
            let (t, d) = (fty.clone(), fdesc.clone());
            let (cj2, fname2, fdesc2) = (cj.clone(), fname.clone(), fdesc.clone());
            b.add_code(
                ACC_PUBLIC | ACC_STATIC,
                "equals$extension",
                &format!("({d}Ljava/lang/Object;)Z"),
                fslots + 1,
                move |asm| {
                    let no = asm.fresh_label();
                    asm.aload(fslots);
                    asm.instanceof(&cj2);
                    asm.ifeq(no);
                    load(asm, 0, jvm_sort(&t));
                    asm.aload(fslots);
                    asm.checkcast(&cj2);
                    asm.getfield(&cj2, &fname2, &fdesc2);
                    emit_field_ne_jump(asm, &t, no);
                    asm.iconst(1);
                    asm.ireturn();
                    asm.mark(no);
                    asm.iconst(0);
                    asm.ireturn();
                },
            );
        }
        if !defined.contains("equals") {
            let (cj2, fname2, fdesc2, d) =
                (cj.clone(), fname.clone(), fdesc.clone(), fdesc.clone());
            let cj3 = cj.clone();
            b.add_code(
                ACC_PUBLIC,
                "equals",
                "(Ljava/lang/Object;)Z",
                2,
                move |asm| {
                    asm.aload(0);
                    asm.getfield(&cj2, &fname2, &fdesc2);
                    asm.aload(1);
                    asm.invokestatic(
                        &cj3,
                        "equals$extension",
                        &format!("({d}Ljava/lang/Object;)Z"),
                    );
                    asm.ireturn();
                },
            );
        }
    }
}

/// The value of a `@SerialVersionUID(n)` annotation on a template, if it has
/// one and `n` is a constant this can fold. nsc runs the argument through the
/// full constant folder; `10L + 3L` (`run/t6988`) is the shape that actually
/// appears, so the arithmetic below covers the operators a `Long` constant
/// expression can use and nothing else. An argument it cannot fold leaves the
/// class exactly as it was before: no field, and the JVM's computed UID.
pub(crate) fn serial_version_uid(mods: &scala_rs_parser::Modifiers) -> Option<i64> {
    for a in &mods.annotations {
        if !matches!(
            a.annotation_path().as_str(),
            "SerialVersionUID" | "scala.SerialVersionUID"
        ) {
            continue;
        }
        if let TreeKind::Apply { args, .. } = &a.kind {
            if let [arg] = args.as_slice() {
                return const_long(arg);
            }
        }
    }
    None
}

fn const_long(t: &Tree) -> Option<i64> {
    if let scala_rs_parser::Type::Constant(l) = &t.ty {
        if let Some(v) = lit_long(l) {
            return Some(v);
        }
    }
    match &t.kind {
        TreeKind::Literal { lit } => lit_long(lit),
        TreeKind::Typed { expr, .. } => const_long(expr),
        TreeKind::Apply { fun, args } => {
            let TreeKind::Select { qual, name } = &fun.kind else {
                return None;
            };
            let l = const_long(qual)?;
            match args.as_slice() {
                [] => match name.as_str() {
                    "unary_-" => Some(l.wrapping_neg()),
                    "unary_~" => Some(!l),
                    "toLong" | "toInt" => Some(l),
                    _ => None,
                },
                [r] => {
                    let r = const_long(r)?;
                    match name.as_str() {
                        "+" => Some(l.wrapping_add(r)),
                        "-" => Some(l.wrapping_sub(r)),
                        "*" => Some(l.wrapping_mul(r)),
                        "/" if r != 0 => Some(l.wrapping_div(r)),
                        "%" if r != 0 => Some(l.wrapping_rem(r)),
                        "|" => Some(l | r),
                        "&" => Some(l & r),
                        "^" => Some(l ^ r),
                        "<<" => Some(l.wrapping_shl(r as u32)),
                        ">>" => Some(l.wrapping_shr(r as u32)),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn lit_long(l: &scala_rs_parser::Lit) -> Option<i64> {
    match l {
        scala_rs_parser::Lit::Long(v) => Some(*v),
        scala_rs_parser::Lit::Int(v) => Some(*v as i64),
        _ => None,
    }
}
