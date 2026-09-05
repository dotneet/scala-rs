//! `object`s, companions, and the members that live on or beside them: the
//! module classfile with its `MODULE$` and mixin initialisers, the companion
//! of a `case` or value class, static forwarders, and the emitters for a
//! `case` class's `apply` / `unapply` / `copy` / `productN`.

use crate::classfile::{
    encode_method_name, Field, ACC_FINAL, ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
};
use crate::code::Assembler;
use crate::companion_fwd::{self, Forwarder};
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{SymKind, SymbolTable};
use std::collections::HashSet;

impl<'a> Gen<'a> {
    /// `value_class`: the `ClassDef` of the value class this module is the
    /// companion of, when the source wrote one. Its `$extension` methods are
    /// declared on this module (see `emit_value_companion`), so the forwarders
    /// have to land here rather than on a synthesized companion.
    pub(crate) fn emit_module(
        &mut self,
        tree: &Tree,
        class_names: &HashSet<String>,
        value_class: Option<&Tree>,
    ) {
        let lambda_wm = self.lambda_watermark();
        let (name, mods, impl_) = match &tree.kind {
            TreeKind::ModuleDef { name, mods, impl_ } => (name, mods, impl_),
            _ => return,
        };
        let m = tree.sym;
        let cls = if m.is_none() {
            m
        } else {
            module_class_id(self.st, m)
        };
        let this_name = if cls.is_none() {
            format!("{name}$")
        } else {
            class_internal(self.st, cls)
        };

        // An `object` that is a member of a class or trait has one instance
        // per enclosing instance, reached through the enclosing template's
        // `<name>()` accessor — not a static `MODULE$` singleton.
        let inner_outer = member_module_outer(self.st, cls);
        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = if inner_outer.is_some() {
            ACC_PUBLIC | ACC_SUPER
        } else {
            ACC_PUBLIC | ACC_FINAL | ACC_SUPER
        };
        let (super_name, interfaces) = split_parents(self.st, &impl_.parents);
        b.super_name = super_name;
        b.interfaces = interfaces;
        match companion_case_class(self.st, cls) {
            // A case class's companion the *user* wrote. nsc gives it
            // `Serializable` and nothing else -- not even `AbstractFunctionN`,
            // whatever it extends (`object WithTrait extends Mix` comes out as
            // `class E$WithTrait$ implements E$Mix, java.io.Serializable`).
            Some(_) => self.add_serializable(&mut b),
            // `case object Q`: a `Product` in its own right.
            None if mods.flags.contains(Flags::CASE) => self.add_product_interfaces(&mut b),
            None => {}
        }
        // A member `object` lives once per enclosing instance, so it carries
        // an `$outer` instead of the singleton `MODULE$`.
        if let Some(o) = inner_outer {
            let outer_desc = outer_field_desc(self.st, cls)
                .unwrap_or_else(|| format!("L{};", class_internal(self.st, o)));
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: "$outer".into(),
                desc: outer_desc,
            });
        } else {
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
                name: "MODULE$".into(),
                desc: format!("L{this_name};"),
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
            }
        }
        for (name, ty, extra) in self.mixin_val_fields(cls, &[], &impl_.body) {
            b.fields.push(Field {
                access: ACC_PUBLIC | extra,
                name,
                desc: jvm_desc_val(self.st, &ty),
            });
        }
        let lazies = self.all_lazy_vals(cls, &impl_.body);
        let binary_lazies = self.binary_mixin_lazy_vals(cls, &impl_.body);
        for v in &self.mixin_lazy_vals(cls, &impl_.body) {
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

        self.emit_module_init(
            &mut b,
            cls,
            &impl_.body,
            &impl_.parents,
            inner_outer,
            Some(cls),
        );
        if inner_outer.is_none() {
            self.emit_module_clinit(&mut b);
        }
        let own_modules = self.member_modules_of(cls, &impl_.body);
        let mixin_modules = self.mixin_member_modules(cls, &own_modules);
        self.emit_member_module_accessors(&mut b, &own_modules);
        self.emit_member_module_accessors(&mut b, &mixin_modules);
        self.emit_trait_outer_accessors(&mut b, cls);
        self.emit_lazy_accessors(&mut b, cls, &lazies, &binary_lazies);
        self.emit_val_getters(&mut b, &impl_.body);

        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, cls, stt);
            }
        }
        // An `object` mixing in a trait needs the same mixin forwarders a
        // class gets, or its concrete trait methods stay abstract — and the
        // same getter/`$init$set$` pair for the trait's `val`s.
        self.emit_trait_val_accessors(&mut b, cls, &impl_.body);
        // `emit_class` also implements every mixed-in trait's abstract
        // `T$$super$m` accessors here (`emit_super_accessors`); this path
        // never did, so `object Impl extends Mid` (`Mid` itself calling
        // `super.m` from its own overriding body) linked but threw
        // `AbstractMethodError` at the first call -- the interface declared
        // `Mid$$super$m` abstractly (every trait with a `super` call in its
        // body gets one, whether it is ultimately mixed into a `class` or an
        // `object`) and no concrete class ever provided it.
        self.emit_super_accessors(&mut b, cls);
        self.emit_mixin_forwarders(&mut b, cls, &impl_.body);
        self.emit_delayed_init_support(&mut b, cls, &impl_.body, true);
        // `object Main extends App`: `main` is the one forwarder the module's
        // own method table cannot supply -- the body lives on the `App` trait
        // and reaches the module through the interface, not through a method
        // of its own. nsc's mirror class has it all the same.
        let mut extra: Vec<(String, String)> = Vec::new();
        if !cls.is_none() && extends_app(self.st, cls) {
            extra.push(("main".into(), "([Ljava/lang/String;)V".into()));
        }
        self.emit_default_getters(&mut b, cls);

        // case-class companion: synthetic apply
        let mut suppressed: HashSet<String> = HashSet::new();
        if let Some(class_id) = self.find_class_named(name) {
            if self.st.get(class_id).flags.contains(Flags::CASE)
                && !impl_.body.iter().any(|t| t.name() == Some("apply"))
            {
                emit_case_apply(&mut b, self.st, class_id);
                // nsc emits no forwarder for an `apply` that is not public.
                // With `-Xsource-features:case-apply-copy-access` the `public
                // static C apply(int)` on the case class itself disappears for
                // both `private` and `private[p]`. `emit_case_apply` writes the
                // method `public` either way, so the access has to be read off
                // the symbol here.
                let apply_sym = case_apply_sym(self.st, class_id);
                if !apply_sym.is_none()
                    && (self.st.get(apply_sym).flags.contains(Flags::PRIVATE)
                        || self.st.get(apply_sym).private_within.is_some())
                {
                    suppressed.insert("apply".into());
                }
            }
            if self.st.get(class_id).flags.contains(Flags::CASE)
                && !impl_.body.iter().any(|t| t.name() == Some("unapply"))
            {
                emit_case_unapply(&mut b, self.st, class_id, self.library_abi);
            }
        }

        // `case object Asc`: nsc's `toString` / `hashCode` / `productPrefix`
        // live on the module class, not on a companion.
        self.emit_case_object_methods(&mut b, cls);

        // `case object Asc extends Direction { override def reverse: Desc.type
        // = Desc }`: a module overriding with a narrower result type needs the
        // same bridge a class gets, or the parent's signature stays abstract.
        self.emit_erasure_bridges(&mut b, cls);
        self.emit_inherited_covariant_bridges(&mut b, cls);
        // A written companion of a value class holds that class's
        // `$extension` methods, exactly as a synthesized one does.
        if let Some(vc) = value_class {
            if let TreeKind::ClassDef { impl_, .. } = &vc.kind {
                self.emit_value_extension_forwarders(&mut b, vc.sym, &impl_.body);
            }
        }
        self.emit_binary_parent_bridges(&mut b, cls);
        self.drain_lambdas(&mut b, lambda_wm);
        attach_scala_sig(&mut b, self.st, cls, &self.pickles);
        b.sign_class(self.sig_of(cls));

        let top_level = if cls.is_none() {
            true
        } else {
            matches!(
                self.st.get(self.st.get(cls).owner).kind,
                SymKind::Package | SymKind::NoSymbol
            )
        };
        // The forwarder set has to be read off `b` before the classfile is
        // written, since that is the only complete list of what the module
        // really carries -- `val` getters, a `var`'s setter, the mixin
        // forwarders a trait's concrete members produce, all of which nsc
        // forwards and none of which is a `DefDef` in the body.
        let mut forwarded = if top_level {
            self.module_forwarders(&b, cls, &extra)
        } else {
            Vec::new()
        };
        forwarded.retain(|f| !suppressed.contains(&f.name));
        self.out
            .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));

        if !top_level {
            return;
        }
        if class_names.contains(name) {
            // A companion class exists, so there is no mirror class: nsc puts
            // the same forwarders on the companion's own classfile. Without
            // them `object Test { def main(…) }` next to `class Test` left
            // `Test.class` with no `main` at all and `java Test` could not
            // start it (`scala/scala`'s `run/t363`).
            self.deliver_companion_forwarders(strip_module_dollar(&this_name), forwarded);
        } else {
            // A package object needs its mirror class too. nsc compiles
            // `package object p` to *two* classfiles, `p/package$.class` (the
            // module) and `p/package.class` (the mirror), and the mirror is
            // where it puts the `ScalaSignature`: `package$.class` carries
            // only the bare `Scala` marker attribute. Without `p/package.class`
            // a separately compiled consumer finds no pickle for the package
            // object at all, and every one of its members is invisible -- real
            // scalac reading a scala-rs build of a package object said `object
            // twice is not a member of package myp.util` for each of them. See
            // `docs/notes/companions-and-class-symbols.md`.
            self.emit_forwarder(&this_name, &forwarded, cls);
        }
    }

    /// `<Iface>.$init$(this)` for every mixed-in trait that has `val`s to
    /// set, in reverse linearization order (base traits first). `$init$` is a
    /// `static` method on the interface itself, so the call is an
    /// `invokestatic` through an `InterfaceMethodref`.
    pub(crate) fn mixin_init_calls(&self, class_id: SymbolId) -> Vec<(String, String)> {
        if class_id.is_none() {
            return Vec::new();
        }
        linearize(self.st, class_id)
            .into_iter()
            .skip(1)
            .rev()
            .filter_map(|p| {
                if !is_interface_sym(self.st, p) {
                    return None;
                }
                // A trait of this run: call `$init$` when it has something to
                // run. A trait read from `-cp`: call it when the interface
                // declares one, which is nsc's own rule for a binary trait.
                // Reading only `TraitImpls::inits` left every `val` of a
                // classpath trait at its default -- no exception, no
                // diagnostic, just a `null`.
                if !self.traits.inits.contains_key(&p)
                    && (self.traits.impls.contains_key(&p) || !self.declares_mixin_ctor(p))
                {
                    return None;
                }
                let iface = class_internal(self.st, p);
                Some((iface.clone(), format!("(L{iface};)V")))
            })
            .collect()
    }

    /// Does this trait's *signature* carry `$init$`? True only for a trait
    /// read from a class file: one compiled in this run is answered from
    /// [`TraitImpls`] instead, which knows whether the `$init$` we emit has a
    /// body worth calling.
    pub(crate) fn declares_mixin_ctor(&self, trait_id: SymbolId) -> bool {
        self.st
            .get(trait_id)
            .members
            .iter()
            .any(|&m| self.st.get(m).name == "$init$")
    }

    pub(crate) fn emit_module_init(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
        parents: &[Tree],
        inner_outer: Option<SymbolId>,
        // The class whose linearization decides which trait `$init$`s run.
        // For a `case class`'s synthetic companion this is the *module*
        // class, not the case class `class_id` names.
        mixin_owner: Option<SymbolId>,
    ) {
        let class_name = b.this_name.clone();
        let st = self.st;
        // `val` initializers *and* the body's bare statements, in source order.
        let inits: Vec<&Tree> = template_init_stats(body);
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let lambda_bodies = &self.lambda_bodies;
        let hoist_owner = b.this_name.clone();
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let delayed = extends_delayed_init(st, class_id);
        let is_app = extends_app(st, class_id);
        let super_name = b.super_name.clone();
        // `object X extends Y(args)` / `case object X extends Y(args)`: the
        // module's own private `<init>` always takes no arguments, but the
        // *super* constructor it invokes must carry the parent's actual
        // constructor arguments, exactly like an ordinary class `<init>`
        // (see the `parent_super_ctor` use a few hundred lines up for
        // `emit_class`). Previously this always emitted a no-arg
        // `invokespecial(super, "<init>", "()V")`, which crashed at runtime
        // (`NoSuchMethodError`) for any singleton extending a class whose
        // primary constructor takes parameters — e.g. slick's
        // `case object Asc extends Direction(false)`.
        let (super_owner, super_desc, super_args, super_cls, super_field_tys) =
            parent_super_ctor(st, parents, &super_name);
        let super_outer = outer_field_class(st, super_cls);
        let mixin_inits = self.mixin_init_calls(mixin_owner.unwrap_or(class_id));
        // A member `object` of a class or trait takes the enclosing instance
        // and keeps it in `$outer`; there is no static `MODULE$` to publish.
        let own_outer = inner_outer.map(|o| {
            outer_field_desc(st, class_id).unwrap_or_else(|| format!("L{};", class_internal(st, o)))
        });
        let (acc, ctor_desc, max_locals) = match &own_outer {
            Some(d) => (ACC_PUBLIC, format!("({d})V"), 4),
            None => (ACC_PRIVATE, "()V".to_string(), 4),
        };
        let outer_cls = inner_outer.unwrap_or(SymbolId::NONE);
        b.add_code(acc, "<init>", &ctor_desc, max_locals, |asm| {
            let mut frame = Frame::instance();
            if own_outer.is_some() {
                frame.next_slot += 1; // slot 1 is $outer
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
            );
            if own_outer.is_some() {
                // nsc rejects a null enclosing instance up front.
                asm.aload(1);
                let ok = asm.fresh_label();
                asm.ifnonnull(ok);
                asm.aconst_null();
                asm.athrow();
                asm.mark(ok);
            }
            // Before the super call, as nsc does (see `emit_class_ctor`).
            if let Some(d) = &own_outer {
                asm.aload(0);
                asm.aload(1);
                asm.putfield(&class_name, "$outer", d);
            }
            // `this` is `uninitializedThis` until the super call returns, so
            // an enclosing-instance read in a super-constructor argument has
            // to come from the `<init>` parameter, not from `$outer`.
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
            );
            if own_outer.is_some() {
                ctx_early.presuper_outer = presuper_outer_of(st, class_id);
            }
            asm.aload(0);
            if let Some(o) = super_outer {
                // Read the enclosing instance out of the argument when it is
                // the instance the parent wants.
                if own_outer.is_some() && is_owner_compatible(st, outer_cls, o) {
                    asm.aload(1);
                } else {
                    load_outer_arg(asm, &ctx_early, o);
                }
            }
            for (i, a) in super_args.iter().enumerate() {
                gen_expr(asm, &mut frame, &ctx_early, a);
                adapt_unit_arg(asm, &ctx_early, a, &a.ty);
                // `object O extends AtomicReference[Int](1)`: same generic /
                // Java superclass boxing `gen_new` and the class `<init>`
                // path above apply.
                let pty = super_field_tys.get(i).unwrap_or(&a.ty);
                if is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty) {
                    emit_box(asm, &a.ty);
                }
            }
            asm.invokespecial(&super_owner, "<init>", &super_desc);
            if own_outer.is_none() {
                asm.aload(0);
                asm.putstatic(&class_name, "MODULE$", &format!("L{class_name};"));
            }
            for (iface, init_desc) in &mixin_inits {
                asm.aload(0);
                asm.invokestatic_interface(iface, "$init$", init_desc);
            }
            if delayed {
                if library_abi && is_app {
                    asm.aload(0);
                    asm.invokestatic_interface("scala/App", "$init$", "(Lscala/App;)V");
                }
                Gen::emit_delayed_init_call(asm, &class_name);
            } else {
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
                        let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                            st.get(vd.sym).ty.clone()
                        } else {
                            vd.ty.clone()
                        };
                        emit_putfield_from_expr(asm, &class_name, name, &jvm_desc_val(st, &ty));
                    } else {
                        // A bare statement of the module body (SLS 5.1): part
                        // of module initialization, so it runs exactly once,
                        // when `MODULE$` is created.
                        gen_stat(asm, &mut frame, &ctx, vd);
                    }
                }
            }
            asm.vreturn();
        });
    }

    pub(crate) fn emit_module_clinit(&self, b: &mut ClassBuilder) {
        let class_name = b.this_name.clone();
        b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
            asm.new_obj(&class_name);
            asm.dup();
            asm.invokespecial(&class_name, "<init>", "()V");
            asm.pop();
            asm.vreturn();
        });
    }

    /// nsc's `extmethods` phase moves a value class's methods to
    /// `name$extension` statics *and declares them on the class's companion
    /// module*, synthesizing that module when the source did not write one.
    /// Every call site nsc compiles then reads them there:
    /// `Ops$.MODULE$.inc$extension(x)`.
    ///
    /// We put the statics on the value class itself, which our own call sites
    /// use (`invoke_value_extension`) and which is fine inside one run. It is
    /// not fine across runs: real scalac compiling `new myp.Ops(41).inc`
    /// against a scala-rs build of `Ops` crashed at its erasure phase with
    /// `AssertionError: no extension method found for: method inc:Int`,
    /// because `myp.Ops$` was not there to look in. `extmethods` runs *before*
    /// `pickler`, so the extension methods are part of the pickle scalac
    /// reads, not something it recovers from the classfile's method table --
    /// `value_companion::add_value_class_companions` declares them, and this
    /// writes the classfile they name.
    ///
    /// The module's methods forward to the statics rather than repeating the
    /// bodies, so there is one copy of each and both ABIs work.
    pub(crate) fn emit_value_companion(&mut self, class_tree: &Tree) {
        let class_id = class_tree.sym;
        if class_id.is_none() || !self.st.is_value_class(class_id) {
            return;
        }
        let TreeKind::ClassDef { impl_, .. } = &class_tree.kind else {
            return;
        };
        // Only where `value_companion::add_value_class_companions` declared
        // one, so the classfile and the pickle always agree. It declines the
        // shapes SLS 3.2.10 forbids and we do not yet reject -- a value class
        // that is local, or a member of a class rather than of an object.
        let Some(comp) = self
            .st
            .companion_module(class_id)
            .map(|m| module_class_id(self.st, m))
        else {
            return;
        };
        let this_name = format!("{}$", class_internal(self.st, class_id));
        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        b.fields.push(Field {
            access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
            name: "MODULE$".into(),
            desc: format!("L{this_name};"),
        });
        self.emit_module_init(&mut b, comp, &[], &[], None, Some(comp));
        self.emit_module_clinit(&mut b);
        self.emit_value_extension_forwarders(&mut b, class_id, &impl_.body);
        // The value class's own pickle, which describes the companion too --
        // the same thing `emit_case_companion` attaches. A Scala classfile
        // with no signature at all is read as a *Java* class, and the Java
        // symbol then collides with the `object` the class's pickle declares.
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        self.out
            .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
    }

    /// One instance method per `$extension` static on the value class, with
    /// the same descriptor, forwarding to it. Kept in step with what
    /// `emit_value_extension` and `emit_value_class_methods` put on the class.
    pub(crate) fn emit_value_extension_forwarders(
        &mut self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
    ) {
        let class_jvm = class_internal(self.st, class_id);
        let Some(under) = self.st.value_class_underlying(class_id) else {
            return;
        };
        let udesc = jvm_desc_val(self.st, &under);
        let mut todo: Vec<(String, String)> = Vec::new();
        for stt in body {
            let TreeKind::DefDef { name, rhs, .. } = &stt.kind else {
                continue;
            };
            if rhs.is_empty() || name == "<init>" || name == "<clinit>" {
                continue;
            }
            // Raw name: `add_code` and `invokestatic` both run it through
            // `encode_method_name`, exactly as `emit_value_extension` does
            // for the static this forwards to.
            todo.push((
                format!("{name}$extension"),
                value_extension_desc(self.st, stt.sym),
            ));
        }
        // A default getter is reached through an `$extension` static of its
        // own (`emit_default_getters` emits one beside the getter), and it is
        // a synthesized *symbol* -- there is no `DefDef` in `body` for the
        // loop above to find. slick's
        // `StringColumnExtensionMethods.like(pattern)` leaves `esc: Char = ' '`
        // out and got
        // `NoSuchMethodError: StringColumnExtensionMethods$.like$default$2$extension`.
        for mid in self.st.get(class_id).members.clone() {
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
            let mut tys = vec![under.clone()];
            tys.extend(pts);
            let name = format!("{}$extension", s.name);
            if todo.iter().any(|(m, _)| *m == name) {
                continue;
            }
            todo.push((name, jvm_method_desc(self.st, &tys, &ret)));
        }
        for (n, d) in [
            ("hashCode$extension", format!("({udesc})I")),
            ("equals$extension", format!("({udesc}Ljava/lang/Object;)Z")),
        ] {
            if !todo.iter().any(|(m, _)| m == n) {
                todo.push((n.to_string(), d));
            }
        }
        for (name, desc) in todo {
            let sorts = desc_param_sorts(&desc);
            let ret = desc_ret_sort(&desc);
            // Slot 0 is the module instance; the static's arguments start at 1.
            let mut slot = 1u16;
            let mut loads = Vec::new();
            for s in &sorts {
                loads.push((slot, *s));
                slot += s.slots();
            }
            let owner = class_jvm.clone();
            let dcopy = desc.clone();
            let ncopy = name.clone();
            b.add_code(
                ACC_PUBLIC | ACC_FINAL,
                &name,
                &desc,
                slot.max(1),
                move |asm| {
                    for (sl, so) in &loads {
                        load(asm, *sl, *so);
                    }
                    asm.invokestatic(&owner, &ncopy, &dcopy);
                    ret_of_sort(asm, ret);
                },
            );
        }
    }

    pub(crate) fn emit_case_companion(&mut self, class_tree: &Tree) {
        let lambda_wm = self.lambda_watermark();
        let class_id = class_tree.sym;
        let class_jvm = if class_id.is_none() {
            class_tree.name().unwrap_or("X").to_string()
        } else {
            class_internal(self.st, class_id)
        };
        let this_name = format!("{class_jvm}$");
        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        // A case class nested in a class has a nested companion too: it gets
        // the same `$outer` and per-enclosing-instance accessor.
        let comp = self
            .st
            .companion_module(class_id)
            .map(|m| module_class_id(self.st, m));
        let inner_outer = comp.and_then(|c| member_module_outer(self.st, c));
        b.access = if inner_outer.is_some() {
            ACC_PUBLIC | ACC_SUPER
        } else {
            ACC_PUBLIC | ACC_FINAL | ACC_SUPER
        };
        // The superclass the typer picked (`Typer::link_case_companion`), if
        // it gave this companion one: nsc's
        // `class Main$P$ extends scala.runtime.AbstractFunction2`.
        let abs_fn = self.companion_abstract_function(class_id);
        if let Some(sup) = &abs_fn {
            b.super_name = sup.clone();
        }
        self.add_serializable(&mut b);
        match outer_field_desc(self.st, class_id).filter(|_| inner_outer.is_some()) {
            Some(d) => b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: "$outer".into(),
                desc: d,
            }),
            None => b.fields.push(Field {
                access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
                name: "MODULE$".into(),
                desc: format!("L{this_name};"),
            }),
        }
        // The mixin `$init$` calls belong to the *companion module class*,
        // which has no parents of its own beyond `AbstractFunctionN` /
        // `Serializable` -- not to the case class. Passing `class_id` here
        // made `Ap$.<init>` call `N.$init$(this)` for every trait the
        // case class mixes in, and the JVM threw `IncompatibleClassChangeError:
        // class Ap$ does not implement the requested interface N` the first
        // time anything touched the companion (slick: `slick.ast.Apply$`).
        self.emit_module_init(&mut b, class_id, &[], &[], inner_outer, comp);
        if inner_outer.is_none() {
            self.emit_module_clinit(&mut b);
        }
        emit_case_apply(&mut b, self.st, class_id);
        emit_case_unapply(&mut b, self.st, class_id, self.library_abi);
        if abs_fn.is_some() {
            emit_case_apply_bridge(&mut b, self.st, class_id);
        }
        // nsc's `caseModuleToStringMethod`: a case class's companion prints as
        // the class's name. Without it the companion inherited
        // `AbstractFunctionN.toString` and `println(CC)` printed
        // `<function0>`.
        emit_case_companion_to_string(&mut b, self.st, class_id, class_tree);
        // The primary constructor's `$lessinit$greater$default$n` /
        // `apply$default$n` (`crate::typer::ctor_defaults` declares them on the
        // companion, as nsc does). `emit_module` reaches these through its own
        // call; a synthesized companion has no `ModuleDef` and comes here.
        if let Some(c) = comp {
            self.emit_default_getters(&mut b, c);
        }
        // `case class M(v: Int) extends AnyVal`: this synthetic companion is
        // also where its `$extension` methods are declared.
        if self.st.is_value_class(class_id) {
            if let TreeKind::ClassDef { impl_, .. } = &class_tree.kind {
                self.emit_value_extension_forwarders(&mut b, class_id, &impl_.body);
            }
        }
        self.drain_lambdas(&mut b, lambda_wm);
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        self.out
            .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
    }

    /// The `scala/runtime/AbstractFunctionN` a case class's synthetic
    /// companion extends, if the typer linked one
    /// (`crates/typer/src/prelude_product.rs` for when it does).
    pub(crate) fn companion_abstract_function(&self, class_id: SymbolId) -> Option<String> {
        if class_id.is_none() {
            return None;
        }
        let module = self.st.companion_module(class_id)?;
        let cls = module_class_id(self.st, module);
        self.st.get(cls).parents.iter().find_map(|p| {
            let id = self.st.class_sym_of(p)?;
            let jvm = class_internal(self.st, id);
            jvm.starts_with("scala/runtime/AbstractFunction")
                .then_some(jvm)
        })
    }

    /// `scala.Product` and `java.io.Serializable`, the two interfaces nsc puts
    /// on every `case class` and `case object`. Only under `--scala-library`:
    /// the private runtime has no `scala/Product`, and naming an interface
    /// that will not be on the classpath makes the class unloadable.
    pub(crate) fn add_product_interfaces(&self, b: &mut ClassBuilder) {
        if self.library_abi && !b.interfaces.iter().any(|i| i == "scala/Product") {
            b.interfaces.push("scala/Product".into());
        }
        self.add_serializable(b);
    }

    /// `java.io.Serializable` alone -- a case class's companion, which is not
    /// a `Product`. A JDK interface, so it needs no library.
    pub(crate) fn add_serializable(&self, b: &mut ClassBuilder) {
        if !b.interfaces.iter().any(|i| i == "java/io/Serializable") {
            b.interfaces.push("java/io/Serializable".into());
        }
    }

    pub(crate) fn emit_forwarder(
        &mut self,
        module_jvm: &str,
        methods: &[Forwarder],
        class_id: SymbolId,
    ) {
        let fwd_name = strip_module_dollar(module_jvm);
        let mut b = ClassBuilder::new(fwd_name, self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        add_static_forwarders(&mut b, module_jvm, methods);
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        // `class_id` is the module's own symbol: `fwd_name` (`Main`, no `$`)
        // never matches a symbol's own jvm name, so the self-entry lookup
        // always misses here — but scalac's mirror class still lists the
        // object's own nested classes unconditionally (verified against
        // real scalac's `Main.class`), so pass it through as the fallback
        // "list my members" owner.
        self.out
            .push(b.finish_full(self.st, &self.jvm_index, class_id));
    }

    pub(crate) fn find_class_named(&self, name: &str) -> Option<SymbolId> {
        self.class_by_name.get(name).copied()
    }

    /// nsc's `addForwarders`, for a top-level `object`: which of the methods
    /// just emitted onto its module classfile become `public static`
    /// pass-throughs on the classfile that carries the object's *plain* name.
    /// See [`crate::companion_fwd`] for the rules and how they were measured.
    ///
    /// `extra` is what the old tree-driven list contributed and the method
    /// table cannot: the `main` an `object … extends App` inherits.
    pub(crate) fn module_forwarders(
        &self,
        b: &ClassBuilder,
        module_class: SymbolId,
        extra: &[(String, String)],
    ) -> Vec<Forwarder> {
        let companion = companion_fwd::companion_class_of(self.st, module_class);
        let restricted = companion_fwd::restricted_names(self.st, module_class);
        let conflicting =
            companion_fwd::conflicting_names(self.st, companion.unwrap_or(SymbolId::NONE));
        let mut out = companion_fwd::pick(&b.methods, &restricted, &conflicting);
        for (name, desc) in extra {
            let name = encode_method_name(name);
            if conflicting.contains(&name) || restricted.contains(&name) {
                continue;
            }
            if out.iter().any(|f| f.name == name && &f.desc == desc) {
                continue;
            }
            out.push(Forwarder {
                name,
                desc: desc.clone(),
                signature: None,
            });
        }
        out
    }

    /// Hand a companion class the forwarders its `object` owes it. The two
    /// are emitted in source order, so this is called both before and after
    /// [`Gen::finish_companion_class`] has seen the class.
    pub(crate) fn deliver_companion_forwarders(&mut self, class_jvm: String, fwd: Vec<Forwarder>) {
        match self
            .parked_companions
            .iter()
            .position(|(n, _)| *n == class_jvm)
        {
            Some(i) => {
                let (_, mut b) = self.parked_companions.remove(i);
                add_static_forwarders(&mut b, &format!("{class_jvm}$"), &fwd);
                self.out
                    .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
            }
            None => {
                self.companion_fwd.insert(class_jvm, fwd);
            }
        }
    }

    /// Write a class that may be the companion of a top-level `object`. When
    /// it is, and the `object` has not been emitted yet, the builder waits in
    /// `parked_companions` until it has — a classfile cannot be reopened once
    /// its constant pool is written.
    pub(crate) fn finish_companion_class(&mut self, mut b: ClassBuilder, has_object: bool) {
        if !has_object {
            self.out
                .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
            return;
        }
        let this_name = b.this_name.clone();
        match self.companion_fwd.remove(&this_name) {
            Some(fwd) => {
                add_static_forwarders(&mut b, &format!("{this_name}$"), &fwd);
                self.out
                    .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
            }
            None => self.parked_companions.push((this_name, b)),
        }
    }

    /// Write out any companion class still waiting for an `object` that never
    /// arrived. Nothing should normally be left here, but a dropped classfile
    /// is a far worse failure than a missing forwarder.
    pub(crate) fn flush_parked_companions(&mut self) {
        for (name, mut b) in std::mem::take(&mut self.parked_companions) {
            if let Some(fwd) = self.companion_fwd.remove(&name) {
                add_static_forwarders(&mut b, &format!("{name}$"), &fwd);
            }
            self.out
                .push(b.finish_full(self.st, &self.jvm_index, SymbolId::NONE));
        }
    }
}

/// `scala.Product`'s indexed accessors on a `case class` / `case object`:
/// `productElement`, `productElementName`, `productIterator` and
/// `productElementNames`.
///
/// Checked against `javap -v -p` on scalac 2.13.16 (`crates/typer/src/prelude_product.rs`
/// records the rest of that reading):
///
/// * `productElement` and `productElementName` are a `tableswitch` over
///   `0 … arity-1` — a *table*, not a chain of comparisons, and one even for a
///   single field (`tableswitch { // 0 to 0 }`). A zero-field case class emits
///   no switch at all, just the out-of-range path.
/// * Both take the out-of-range index to `scala.runtime.Statics.ioobe(I)`,
///   which throws `IndexOutOfBoundsException(String.valueOf(i))`;
///   `productElementName` follows it with `checkcast java/lang/String`, since
///   `ioobe` is declared to return `Object`.
/// * `productIterator` is *not* inherited: nsc overrides it with
///   `ScalaRunTime$.MODULE$.typedProductIterator(this)`.
/// * `productElementNames` *is* inherited — the emitted method is the mixin
///   forwarder to `Product`'s default implementation,
///   `scala/Product.productElementNames$(this)`.
///
/// A field of value-class type is read unboxed and wrapped back up, exactly as
/// `toString` does it (nsc: `new G$Meters(this.m())` for
/// `case class Box(m: Meters, b: String)`).
///
/// A `case object` (`module`) is the one exception: nsc synthesizes no
/// `productElementName` for it at all, so the module class ends up with the
/// mixin forwarder to `Product`'s default, whose message is
/// `"0 is out of bounds (min 0, max -1)"` rather than the bare `"0"` a
/// zero-field `case class Zero()` throws through `Statics.ioobe`. Both were
/// read off scalac's own output, and both are reproduced here.
///
/// `productIterator` and `productElementNames` need `scala.collection.Iterator`,
/// `scala.Product` and `scala.runtime.ScalaRunTime`, none of which the private
/// runtime has, so they are emitted only under `--scala-library`; the typer
/// leaves them undeclared in the other mode (`Typer::link_case_product`), so
/// nothing calls what is not there. The other two need only `java.lang`, and
/// are emitted in both modes with the throw written out where `Statics.ioobe`
/// (or `Product`'s default) would go.
pub(crate) fn emit_product_accessors(
    b: &mut ClassBuilder,
    defined: &HashSet<String>,
    class_jvm: &str,
    field_info: &[(String, Type, String)],
    field_vc: &[Option<(String, String)>],
    library_abi: bool,
    module: bool,
) {
    let n = field_info.len();
    if !defined.contains("productElement") {
        let fi = field_info.to_vec();
        let fvc = field_vc.to_vec();
        let cj = class_jvm.to_string();
        b.add_code(
            ACC_PUBLIC,
            "productElement",
            "(I)Ljava/lang/Object;",
            2,
            move |asm| {
                let dflt = asm.fresh_label();
                let labs: Vec<crate::code::Label> = (0..n).map(|_| asm.fresh_label()).collect();
                if n > 0 {
                    asm.iload(1);
                    asm.tableswitch(dflt, 0, n as i32 - 1, &labs);
                    for (i, (name, ty, desc)) in fi.iter().enumerate() {
                        asm.mark(labs[i]);
                        match fvc.get(i) {
                            Some(Some((internal, ctor))) => {
                                asm.new_obj(internal);
                                asm.dup();
                                asm.aload(0);
                                asm.getfield(&cj, name, desc);
                                asm.invokespecial(internal, "<init>", ctor);
                            }
                            _ => {
                                asm.aload(0);
                                asm.getfield(&cj, name, desc);
                                if is_jvm_primitive(ty) && !erases_to_boxed_unit(ty) {
                                    emit_box(asm, ty);
                                }
                            }
                        }
                        asm.areturn();
                    }
                }
                asm.mark(dflt);
                emit_ioobe(asm, library_abi, false);
            },
        );
    }
    if !defined.contains("productElementName") {
        let names: Vec<String> = field_info.iter().map(|(nm, _, _)| nm.clone()).collect();
        b.add_code(
            ACC_PUBLIC,
            "productElementName",
            "(I)Ljava/lang/String;",
            3,
            move |asm| {
                if module {
                    emit_default_product_element_name(asm, library_abi, n as i32);
                    return;
                }
                let dflt = asm.fresh_label();
                let labs: Vec<crate::code::Label> = (0..n).map(|_| asm.fresh_label()).collect();
                if n > 0 {
                    asm.iload(1);
                    asm.tableswitch(dflt, 0, n as i32 - 1, &labs);
                    for (i, nm) in names.iter().enumerate() {
                        asm.mark(labs[i]);
                        asm.ldc_string(nm);
                        asm.areturn();
                    }
                }
                asm.mark(dflt);
                emit_ioobe(asm, library_abi, true);
            },
        );
    }
    if !library_abi {
        return;
    }
    if !defined.contains("productIterator") {
        b.add_code(
            ACC_PUBLIC,
            "productIterator",
            "()Lscala/collection/Iterator;",
            1,
            |asm| {
                asm.getstatic(
                    "scala/runtime/ScalaRunTime$",
                    "MODULE$",
                    "Lscala/runtime/ScalaRunTime$;",
                );
                asm.aload(0);
                asm.invokevirtual(
                    "scala/runtime/ScalaRunTime$",
                    "typedProductIterator",
                    "(Lscala/Product;)Lscala/collection/Iterator;",
                );
                asm.areturn();
            },
        );
    }
    if !defined.contains("productElementNames") {
        b.add_code(
            ACC_PUBLIC,
            "productElementNames",
            "()Lscala/collection/Iterator;",
            1,
            |asm| {
                asm.aload(0);
                asm.invokestatic_interface(
                    "scala/Product",
                    "productElementNames$",
                    "(Lscala/Product;)Lscala/collection/Iterator;",
                );
                asm.areturn();
            },
        );
    }
}

/// A `case object`'s `productElementName`: nsc synthesizes none, so the module
/// class carries the mixin forwarder to `scala.Product`'s default. Every index
/// is out of range for a zero-arity `Product`, and that default's message is
/// `"<n> is out of bounds (min 0, max <arity - 1>)"`.
///
/// Without the jar the same message is built here, so the two library modes do
/// not disagree about what a `case object` throws.
pub(crate) fn emit_default_product_element_name(
    asm: &mut Assembler,
    library_abi: bool,
    arity: i32,
) {
    if library_abi {
        asm.aload(0);
        asm.iload(1);
        asm.invokestatic_interface(
            "scala/Product",
            "productElementName$",
            "(Lscala/Product;I)Ljava/lang/String;",
        );
        asm.areturn();
        return;
    }
    asm.new_obj("java/lang/IndexOutOfBoundsException");
    asm.dup();
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    asm.iload(1);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(I)Ljava/lang/StringBuilder;",
    );
    append_str(asm, " is out of bounds (min 0, max ");
    asm.iconst(arity - 1);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(I)Ljava/lang/StringBuilder;",
    );
    append_str(asm, ")");
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
    asm.invokespecial(
        "java/lang/IndexOutOfBoundsException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    asm.athrow();
}

/// The out-of-range arm of `productElement` / `productElementName`, return
/// included. `as_string` adds the `checkcast` the `String`-returning one needs.
///
/// `scala.runtime.Statics.ioobe` is what nsc calls, and it is exactly
/// `throw new IndexOutOfBoundsException(String.valueOf(i))` (read back from
/// the library jar with `javap -c`). It is declared to return `Object`, so nsc
/// writes `areturn` after the call even though it never returns; without the
/// jar the throw is written out here instead, and then there is nothing to
/// return from.
pub(crate) fn emit_ioobe(asm: &mut Assembler, library_abi: bool, as_string: bool) {
    if library_abi {
        asm.iload(1);
        asm.invokestatic("scala/runtime/Statics", "ioobe", "(I)Ljava/lang/Object;");
        if as_string {
            asm.checkcast("java/lang/String");
        }
        asm.areturn();
        return;
    }
    asm.new_obj("java/lang/IndexOutOfBoundsException");
    asm.dup();
    asm.iload(1);
    asm.invokestatic("java/lang/String", "valueOf", "(I)Ljava/lang/String;");
    asm.invokespecial(
        "java/lang/IndexOutOfBoundsException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    asm.athrow();
}

/// The `case class` a module class is the companion of, if any.
///
/// That is what tells a case class's companion (`P$` next to `P`) from a
/// `case object`'s module class (`Q$` alone). The `CASE` flag alone does not:
/// `Typer::ensure_companion` stamps it on the companion it synthesizes too.
pub(crate) fn companion_case_class(st: &SymbolTable, cls: SymbolId) -> Option<SymbolId> {
    if cls.is_none() {
        return None;
    }
    let name = st.get(cls).name.clone();
    let base = name.strip_suffix('$').unwrap_or(&name).to_string();
    let owner = st.get(cls).owner;
    st.get(owner).members.iter().copied().find(|&m| {
        st.get(m).kind == SymKind::Class
            && st.get(m).name == base
            && st.get(m).flags.contains(Flags::CASE)
    })
}

/// nsc's `caseModuleToStringMethod`: the companion module of a `case class`
/// answers `toString` with the class's own name, so `println(C)` prints `C`
/// and not the `AbstractFunctionN` it happens to extend.
pub(crate) fn emit_case_companion_to_string(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    class_id: SymbolId,
    class_tree: &Tree,
) {
    if b.methods.iter().any(|m| m.name == "toString") {
        return;
    }
    let text = if class_id.is_none() {
        class_tree.name().unwrap_or("X").to_string()
    } else {
        st.get(class_id).name.clone()
    };
    b.add_code(
        ACC_PUBLIC,
        "toString",
        "()Ljava/lang/String;",
        1,
        move |asm| {
            asm.ldc_string(&text);
            asm.areturn();
        },
    );
}

/// `apply(Object, …): Object`, the erased `FunctionN.apply` a companion that
/// extends `scala.runtime.AbstractFunctionN` has to implement. nsc emits it as
/// `public java.lang.Object apply(java.lang.Object, java.lang.Object)` right
/// after the typed `apply`.
pub(crate) fn emit_case_apply_bridge(b: &mut ClassBuilder, st: &SymbolTable, class_id: SymbolId) {
    let fields = st.get(class_id).ctor_fields.clone();
    let tys: Vec<Type> = fields.iter().map(|f| st.get(*f).ty.clone()).collect();
    let ret = Type::Class {
        sym: class_id,
        args: vec![],
    };
    let target = jvm_method_desc(st, &tys, &ret);
    let bridge = format!(
        "({})Ljava/lang/Object;",
        "Ljava/lang/Object;".repeat(tys.len())
    );
    if b.methods
        .iter()
        .any(|m| m.name == "apply" && m.desc == bridge)
    {
        return;
    }
    let this = b.this_name.clone();
    let locals = tys.len() as u16 + 1;
    b.add_code(ACC_PUBLIC, "apply", &bridge, locals, move |asm| {
        asm.aload(0);
        for (i, ty) in tys.iter().enumerate() {
            asm.aload(i as u16 + 1);
            // `Unit` is a `BoxedUnit` reference here, not an unboxed
            // primitive: unboxing it `pop`ped the argument and left the call
            // below with nothing to pass.
            if erases_to_boxed_unit(ty) {
                asm.checkcast(BOXED_UNIT);
            } else if is_jvm_primitive(ty) {
                emit_unbox(asm, ty);
            } else if let Some(internal) = checkcast_internal(st, ty) {
                asm.checkcast(&internal);
            }
        }
        asm.invokevirtual(&this, "apply", &target);
        asm.areturn();
    });
}

pub(crate) fn emit_case_apply(b: &mut ClassBuilder, st: &SymbolTable, class_id: SymbolId) {
    let fields = st.get(class_id).ctor_fields.clone();
    let class_jvm = class_internal(st, class_id);
    // `case class C[T](y: T) extends AnyVal`: the class erases to its
    // underlying type, so nsc's companion `apply` is `(T)T` -- the identity --
    // and not `(T)LC;`. Emitting the boxed shape meant every call site, which
    // *was* type-checked against the erased one, ended in
    // `NoSuchMethodError: 'java.lang.Object C$.apply(java.lang.Object)'`.
    if let Some(under) = value_class_apply_type(st, class_id) {
        let d = jvm_desc_val(st, &under);
        let sort = jvm_slot_sort(&under);
        let desc = format!("({d}){d}");
        if b.methods
            .iter()
            .any(|m| m.name == "apply" && m.desc == desc)
        {
            return;
        }
        let acc = synthetic_case_member_access(st, case_apply_sym(st, class_id));
        b.add_code(acc, "apply", &desc, 1 + sort.slots(), move |asm| {
            load(asm, 1, sort);
            ret_of_sort(asm, sort);
        });
        return;
    }
    let mut params = Vec::new();
    let mut locals = 1u16;
    let mut loads = Vec::new();
    for f in &fields {
        let ty = st.get(*f).ty.clone();
        // Pass-through: the argument the JVM handed us is what goes to the
        // constructor, and a `Unit` one is a `BoxedUnit` reference in a slot.
        let sort = jvm_slot_sort(&ty);
        loads.push((locals, sort));
        locals += sort.slots();
        params.push(ty);
    }
    let ret = Type::Class {
        sym: class_id,
        args: vec![],
    };
    let desc = jvm_method_desc(st, &params, &ret);
    // A case class nested in a class takes its enclosing instance first; the
    // companion is nested in the same class and holds the same one in its own
    // `$outer`. Reading it off the builder keeps the two in step: a companion
    // that is still a static singleton has none to pass.
    let outer = b
        .fields
        .iter()
        .find(|f| f.name == "$outer")
        .map(|f| (b.this_name.clone(), f.desc.clone()));
    let base_ctor_d = jvm_method_desc(st, &params, &Type::Unit);
    let ctor_d = if outer.is_some() {
        with_enclosing_outer_param(st, class_id, &base_ctor_d)
    } else {
        base_ctor_d
    };
    let acc = synthetic_case_member_access(st, case_apply_sym(st, class_id));
    b.add_code(acc, "apply", &desc, locals.max(1), |asm| {
        asm.new_obj(&class_jvm);
        asm.dup();
        if let Some((owner, d)) = &outer {
            asm.aload(0);
            asm.getfield(owner, "$outer", d);
        }
        for (slot, sort) in &loads {
            load(asm, *slot, *sort);
        }
        asm.invokespecial(&class_jvm, "<init>", &ctor_d);
        asm.areturn();
    });
}

/// The erased type a `case class … extends AnyVal`'s companion `apply` and
/// `unapply` speak in, or `None` when `class_id` is not a value class.
///
/// A value class erases to its single field's type, so its companion's
/// synthetic members take and return that type rather than the class.
pub(crate) fn value_class_apply_type(st: &SymbolTable, class_id: SymbolId) -> Option<Type> {
    if !st.is_value_class(class_id) {
        return None;
    }
    let fields = st.get(class_id).ctor_fields.clone();
    if fields.len() != 1 {
        return None;
    }
    st.value_class_underlying(class_id)
        .or_else(|| Some(st.get(fields[0]).ty.clone()))
}

/// One constructor field as `emit_case_unapply` reads it: name, type, field
/// descriptor, and -- when the field's own type is a value class -- the box to
/// wrap it back into (`(internal name, constructor descriptor)`).
pub(crate) type UnapplyField = (String, Type, String, Option<(String, String)>);

/// The synthetic `unapply` of a case class's companion, if the typer made one.
///
/// `Typer::synthesize_case_members` allocates it into the module *class* and
/// also records it on the module value, so both lists are searched.
pub(crate) fn case_unapply_sym(st: &SymbolTable, class_id: SymbolId) -> SymbolId {
    let Some(module) = st.companion_module(class_id) else {
        return SymbolId::NONE;
    };
    let module_cls = st.module_class_of(module);
    let is_it =
        |m: SymbolId| st.get(m).name == "unapply" && st.get(m).flags.contains(Flags::SYNTHETIC);
    st.get(module_cls)
        .members
        .iter()
        .copied()
        .find(|&m| is_it(m))
        .or_else(|| st.get(module).members.iter().copied().find(|&m| is_it(m)))
        .unwrap_or(SymbolId::NONE)
}

/// `unapply(x: C)`, the extractor nsc synthesizes on a case class's companion.
///
/// The pattern matcher does not go through it -- it reads the fields straight
/// off the scrutinee, the way nsc's own optimiser ends up doing -- so this
/// method was never emitted, and every program that named it *itself*
/// (`Foo.unapply(x)`, `foo(Foo.unapply, …)` eta-expanded to a function value)
/// died with `NoSuchMethodError: 'scala.Option Foo$.unapply(Foo)'`.
///
/// Shapes read off `javap -c -p` on scalac 2.13.16:
///
/// * arity 0 -> `public boolean unapply(C)`, `x != null`;
/// * arity 1 -> `public scala.Option unapply(C)`, `if (x == null) None else new Some(x._1)`;
/// * arity n -> the same with `new TupleN(x._1, …, x._n)` inside the `Some`.
///
/// Only the *first* parameter section is extracted, exactly as the typer's own
/// `unapply` signature says (`Typer::finish_case_apply`); the arity is read
/// back off that signature rather than from `ctor_fields`, which is the
/// flattened list.
///
/// A field of value-class type is wrapped back into its box, as
/// `productElement` and `toString` already do.
pub(crate) fn emit_case_unapply(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    class_id: SymbolId,
    library_abi: bool,
) {
    let sym = case_unapply_sym(st, class_id);
    if sym.is_none() {
        return;
    }
    // Only the first parameter section is extracted: nsc leaves the later
    // sections of `case class F(name: String)(val opts: X)` out of the
    // pattern. `ctor_fields` is the flattened list, so the arity comes off the
    // primary constructor. The `unapply` symbol's own result type cannot say:
    // erasure has already dropped its type arguments by the time the backend
    // runs, leaving a bare `Option`.
    let boolean_result = matches!(
        &st.get(sym).ty,
        Type::Method { ret, .. } if matches!(**ret, Type::Boolean)
    );
    let n_fields = st.get(class_id).ctor_fields.len();
    let arity = if boolean_result {
        0
    } else {
        st.get(class_id)
            .members
            .iter()
            .copied()
            // The primary constructor is the one whose sections account for
            // exactly the case fields; an auxiliary `def this` does not.
            .find(|&m| {
                st.get(m).name == "<init>"
                    && st.get(m).paramss.iter().map(|ps| ps.len()).sum::<usize>() == n_fields
            })
            .and_then(|m| st.get(m).paramss.first().map(|ps| ps.len()))
            .unwrap_or(n_fields)
    };
    // A shape the two sides disagree about is one this emitter does not
    // understand; leaving the method out is better than writing one whose
    // descriptor does not match what callers were type-checked against.
    if (arity == 0) != boolean_result {
        return;
    }
    // `TupleN` above 2 is not part of the private runtime, so a wider case
    // class keeps the gap it has today rather than getting a method that
    // cannot link.
    if arity > 2 && !library_abi {
        return;
    }
    let class_jvm = class_internal(st, class_id);
    // A value class's `unapply` takes the *underlying* value and hands that
    // same value back, boxed: nsc's `Wrapped$.unapply(int)` answers
    // `Some(BoxesRunTime.boxToInteger(u))`, never a `Wrapped` (confirmed with
    // `javap -c` on 2.13.16). Call sites here already erase the argument that
    // way -- the failure this replaces was `NoSuchMethodError: 'scala.Option
    // Wrapped$.unapply(int)'`, which names exactly nsc's descriptor -- and the
    // *pattern* form never reaches this method at all, since
    // `gen_ctor_fields_pattern` binds an unboxed value-class scrutinee
    // directly. An earlier attempt was reverted because the pattern path did
    // pass a box; that path is gone.
    if let Some(under) = value_class_apply_type(st, class_id) {
        if arity != 1 {
            return;
        }
        let d = jvm_desc_val(st, &under);
        let sort = jvm_slot_sort(&under);
        let desc = format!("({d})Lscala/Option;");
        if b.methods
            .iter()
            .any(|m| m.name == "unapply" && m.desc == desc)
        {
            return;
        }
        let acc = synthetic_case_member_access(st, sym);
        b.add_code(acc, "unapply", &desc, 1 + sort.slots(), move |asm| {
            asm.new_obj("scala/Some");
            asm.dup();
            load(asm, 1, sort);
            if is_jvm_primitive(&under) && !erases_to_boxed_unit(&under) {
                emit_box(asm, &under);
            }
            asm.invokespecial("scala/Some", "<init>", "(Ljava/lang/Object;)V");
            asm.areturn();
        });
        return;
    }
    let desc = if arity == 0 {
        format!("(L{class_jvm};)Z")
    } else {
        format!("(L{class_jvm};)Lscala/Option;")
    };
    if b.methods
        .iter()
        .any(|m| m.name == "unapply" && m.desc == desc)
    {
        return;
    }
    let fields = st.get(class_id).ctor_fields.clone();
    if fields.len() < arity {
        return;
    }
    let field_info: Vec<UnapplyField> = fields[..arity]
        .iter()
        .map(|f| {
            let s = st.get(*f);
            let vc = st.value_class_terms.get(f).and_then(|&c| {
                let under = st.value_class_underlying(c)?;
                Some((
                    class_internal(st, c),
                    format!("({})V", jvm_desc(st, &under)),
                ))
            });
            (s.name.clone(), s.ty.clone(), jvm_desc_val(st, &s.ty), vc)
        })
        .collect();
    let cj = class_jvm.clone();
    b.add_code(ACC_PUBLIC, "unapply", &desc, 2, move |asm| {
        let nonnull = asm.fresh_label();
        asm.aload(1);
        asm.ifnonnull(nonnull);
        if arity == 0 {
            asm.iconst(0);
            asm.ireturn();
        } else {
            asm.getstatic("scala/None$", "MODULE$", "Lscala/None$;");
            asm.areturn();
        }
        asm.mark(nonnull);
        if arity == 0 {
            asm.iconst(1);
            asm.ireturn();
            return;
        }
        asm.new_obj("scala/Some");
        asm.dup();
        if arity > 1 {
            let tuple = format!("scala/Tuple{arity}");
            asm.new_obj(&tuple);
            asm.dup();
        }
        for (name, ty, d, vc) in &field_info {
            match vc {
                Some((internal, ctor)) => {
                    asm.new_obj(internal);
                    asm.dup();
                    asm.aload(1);
                    asm.getfield(&cj, name, d);
                    asm.invokespecial(internal, "<init>", ctor);
                }
                None => {
                    asm.aload(1);
                    asm.getfield(&cj, name, d);
                    if is_jvm_primitive(ty) && !erases_to_boxed_unit(ty) {
                        emit_box(asm, ty);
                    }
                }
            }
        }
        if arity > 1 {
            let tuple = format!("scala/Tuple{arity}");
            let d = format!("({})V", "Ljava/lang/Object;".repeat(arity));
            asm.invokespecial(&tuple, "<init>", &d);
        }
        asm.invokespecial("scala/Some", "<init>", "(Ljava/lang/Object;)V");
        asm.areturn();
    });
}

/// The case class's own `copy(f1, f2, ...): C = new C(f1, f2, ...)`, mirroring
/// `emit_case_apply` on the companion. Uses the synthetic `copy` method's own
/// parameter symbols (`crate::check::synthesize_case_members` /
/// `type_class` in the typer) rather than `ctor_fields` directly, since those
/// carry `copy`'s own resolved parameter types.
pub(crate) fn emit_case_copy(b: &mut ClassBuilder, st: &SymbolTable, class_id: SymbolId) {
    let Some(copy_id) = st.get(class_id).members.iter().copied().find(|&m| {
        st.get(m).kind == SymKind::Method
            && st.get(m).name == "copy"
            && st.get(m).flags.contains(Flags::SYNTHETIC)
    }) else {
        return;
    };
    let copy_params = st.get(copy_id).params.clone();
    if copy_params.is_empty() {
        return;
    }
    let class_jvm = class_internal(st, class_id);
    let mut params = Vec::new();
    let mut locals = 1u16;
    let mut loads = Vec::new();
    for f in &copy_params {
        let ty = st.get(*f).ty.clone();
        let sort = jvm_slot_sort(&ty);
        loads.push((locals, sort));
        locals += sort.slots();
        params.push(ty);
    }
    let ret = Type::Class {
        sym: class_id,
        args: vec![],
    };
    let desc = jvm_method_desc(st, &params, &ret);
    // `copy` runs on the instance itself, so its own `$outer` is the one the
    // fresh instance gets.
    let ctor_d =
        with_enclosing_outer_param(st, class_id, &jvm_method_desc(st, &params, &Type::Unit));
    let outer = outer_field_desc(st, class_id);
    let acc = synthetic_case_member_access(st, copy_id);
    b.add_code(acc, "copy", &desc, locals.max(1), |asm| {
        asm.new_obj(&class_jvm);
        asm.dup();
        if let Some(d) = &outer {
            asm.aload(0);
            asm.getfield(&class_jvm, "$outer", d);
        }
        for (slot, sort) in &loads {
            load(asm, *slot, *sort);
        }
        asm.invokespecial(&class_jvm, "<init>", &ctor_d);
        asm.areturn();
    });
}

// ---------------------------------------------------------------------------
// bytecode helpers
// ---------------------------------------------------------------------------

/// Jump to `no` when the two values of type `ty` on the stack differ.
pub(crate) fn emit_field_ne_jump(asm: &mut Assembler, ty: &Type, no: crate::code::Label) {
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
        t if is_jvm_primitive(t) => {
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

/// `public static` pass-throughs to `module_jvm`'s `MODULE$`, added to a
/// mirror class or to a companion class.
///
/// Driven by the JVM descriptor rather than by `Type`, so the forwarder moves
/// exactly the slots the target method declares whatever the front end thinks
/// they mean. A descriptor that will not parse, or a method the class already
/// has under the same signature (a value class's `$extension` statics live on
/// the class *and* on the companion), is skipped: a duplicate method makes the
/// whole classfile unloadable.
pub(crate) fn add_static_forwarders(b: &mut ClassBuilder, module_jvm: &str, methods: &[Forwarder]) {
    let module_desc = format!("L{module_jvm};");
    for f in methods {
        let Some((loads, max_locals, ret)) = companion_fwd::desc_slots(&f.desc) else {
            continue;
        };
        let encoded = encode_method_name(&f.name);
        if b.methods
            .iter()
            .any(|m| m.name == encoded && m.desc == f.desc)
        {
            continue;
        }
        let target = f.name.clone();
        let target_desc = f.desc.clone();
        let module_jvm = module_jvm.to_string();
        let module_desc = module_desc.clone();
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            &f.name,
            &f.desc,
            max_locals,
            move |asm| {
                asm.getstatic(&module_jvm, "MODULE$", &module_desc);
                for (slot, sort) in &loads {
                    load(asm, *slot, jvm_sort_of(*sort));
                }
                asm.invokevirtual(&module_jvm, &target, &target_desc);
                ret_of_sort(asm, jvm_sort_of(ret));
            },
        );
        if let Some(sig) = &f.signature {
            if let Some(m) = b.methods.last_mut() {
                m.signature = Some(sig.clone());
            }
        }
    }
}
