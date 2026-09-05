//! Minimal scala-rs runtime classfiles (Java 8 / major 52).
//!
//! These are **not** scala-library. They exist so Option / List / FunctionN
//! from the prelude have JVM types that `scala-rs run` can load.

use crate::classfile::EmittedClass;
use crate::classfile::{
    encode_method_name, ClassEmit, Field, Method, Pool, ACC_ABSTRACT, ACC_FINAL, ACC_INTERFACE,
    ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_SUPER, ACC_VOLATILE,
};
use crate::code::Assembler;

const SRC: &str = "runtime.scala";

/// `scala.runtime.*Ref` boxes, one per JVM sort. A `var` captured by a lambda
/// or by a class defined inside a method is shared through one of these.
const REF_BOXES: &[(&str, &str)] = &[
    ("BooleanRef", "Z"),
    ("ByteRef", "B"),
    ("CharRef", "C"),
    ("DoubleRef", "D"),
    ("FloatRef", "F"),
    ("IntRef", "I"),
    ("LongRef", "J"),
    ("ShortRef", "S"),
    ("ObjectRef", "Ljava/lang/Object;"),
];

/// `scala.runtime.Lazy*`, the one-slot cells a *method-local* `lazy val` is
/// compiled into (nsc's `lazyvals` phase). `LazyUnit` keeps only the flag, so
/// its element descriptor is empty.
const LAZY_CELLS: &[(&str, &str)] = &[
    ("LazyBoolean", "Z"),
    ("LazyByte", "B"),
    ("LazyChar", "C"),
    ("LazyShort", "S"),
    ("LazyInt", "I"),
    ("LazyLong", "J"),
    ("LazyFloat", "F"),
    ("LazyDouble", "D"),
    ("LazyRef", "Ljava/lang/Object;"),
    ("LazyUnit", ""),
];

pub fn emit_runtime() -> Vec<EmittedClass> {
    let mut out = vec![
        emit_function_n(0),
        emit_function_n(1),
        emit_partial_function(),
        emit_ordered(),
        emit_option(),
        emit_some(),
        emit_some_module(),
        emit_none(),
        emit_list(),
        emit_cons(),
        emit_nil(),
        emit_list_module(),
        emit_tuple2(),
        emit_dynamic(),
        emit_arrow_assoc(),
        emit_boxed_unit(),
        emit_nothing(),
        emit_not_implemented(),
        emit_match_error(),
        emit_non_local_return_control(),
        emit_delayed_init(),
        emit_app(),
    ];
    out.extend(REF_BOXES.iter().map(|(n, d)| emit_ref_box(n, d)));
    out.extend(LAZY_CELLS.iter().map(|(n, d)| emit_lazy_cell(n, d)));
    out
}

struct B {
    access: u16,
    this_name: String,
    super_name: String,
    interfaces: Vec<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    pool: Pool,
}

impl B {
    fn class(name: &str, super_name: &str) -> Self {
        B {
            access: ACC_PUBLIC | ACC_SUPER,
            this_name: name.into(),
            super_name: super_name.into(),
            interfaces: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            pool: Pool::new(),
        }
    }

    fn add_code(
        &mut self,
        access: u16,
        name: &str,
        desc: &str,
        max_locals: u16,
        gen: impl FnOnce(&mut Assembler),
    ) {
        let mut asm = Assembler::with_pool(std::mem::take(&mut self.pool), max_locals.max(1));
        asm.init_method(access, name, desc, &self.this_name);
        gen(&mut asm);
        let (code, pool) = asm.finish();
        self.pool = pool;
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: Some(code),
            java_annots: Vec::new(),
        });
    }

    fn add_abstract(&mut self, access: u16, name: &str, desc: &str) {
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: None,
            java_annots: Vec::new(),
        });
    }

    fn finish(self) -> EmittedClass {
        let this_name = self.this_name.clone();
        let class = ClassEmit {
            access: self.access,
            this_name: self.this_name,
            super_name: self.super_name,
            interfaces: self.interfaces,
            fields: self.fields,
            methods: self.methods,
            source: SRC.into(),
            scala_signature: None,
            scala_raw: false,
            inner_classes: Vec::new(),
            enclosing_method: None,
        };
        let bytes = class.write_with_pool(self.pool).expect("runtime classfile");
        EmittedClass {
            internal_name: this_name,
            bytes,
        }
    }
}

fn emit_function_n(n: usize) -> EmittedClass {
    let mut b = B::class(&format!("scala/Function{n}"), "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces.clear();
    let mut desc = String::from("(");
    for _ in 0..n {
        desc.push_str("Ljava/lang/Object;");
    }
    desc.push_str(")Ljava/lang/Object;");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "apply", &desc);
    b.finish()
}

fn emit_partial_function() -> EmittedClass {
    let mut b = B::class("scala/PartialFunction", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces = vec!["scala/Function1".into()];
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "isDefinedAt",
        "(Ljava/lang/Object;)Z",
    );
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "applyOrElse",
        "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
    );
    b.finish()
}

/// `scala.math.Ordered` in nsc 2.13's trait shape: the comparison operators
/// are `default` methods on the interface, each with a `public static m$`
/// beside it taking the receiver. There is no `Ordered$class`.
fn emit_ordered() -> EmittedClass {
    let mut b = B::class("scala/math/Ordered", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces.clear();
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "compare",
        "(Ljava/lang/Object;)I",
    );
    fn cmp_op(b: &mut B, name: &str, jump_true: impl Fn(&mut Assembler, crate::code::Label)) {
        let inst_desc = "(Ljava/lang/Object;)Z";
        let static_desc = "(Lscala/math/Ordered;Ljava/lang/Object;)Z";
        b.add_code(ACC_PUBLIC, name, inst_desc, 2, |asm| {
            asm.aload(0);
            asm.aload(1);
            asm.invokeinterface("scala/math/Ordered", "compare", "(Ljava/lang/Object;)I");
            asm.iconst(0);
            let t = asm.fresh_label();
            let done = asm.fresh_label();
            jump_true(asm, t);
            asm.iconst(0);
            asm.goto(done);
            asm.mark(t);
            asm.iconst(1);
            asm.mark(done);
            asm.ireturn();
        });
        let static_name = format!("{}$", encode_method_name(name));
        let inst_name = name.to_string();
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            &static_name,
            static_desc,
            2,
            move |asm| {
                asm.aload(0);
                asm.aload(1);
                asm.invokespecial_interface("scala/math/Ordered", &inst_name, inst_desc);
                asm.ireturn();
            },
        );
    }
    cmp_op(&mut b, "<", |asm, t| asm.if_icmplt(t));
    cmp_op(&mut b, ">", |asm, t| asm.if_icmpgt(t));
    cmp_op(&mut b, "<=", |asm, t| asm.if_icmple(t));
    cmp_op(&mut b, ">=", |asm, t| asm.if_icmpge(t));
    b.finish()
}

fn emit_option() -> EmittedClass {
    let mut b = B::class("scala/Option", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_ABSTRACT;
    b.add_code(ACC_PUBLIC, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.vreturn();
    });
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "isEmpty", "()Z");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "get", "()Ljava/lang/Object;");

    // map
    b.add_code(
        ACC_PUBLIC,
        "map",
        "(Lscala/Function1;)Lscala/Option;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic("scala/None$", "MODULE$", "Lscala/None$;");
            asm.areturn();
            asm.mark(nonempty);
            asm.new_obj("scala/Some");
            asm.dup();
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.invokespecial("scala/Some", "<init>", "(Ljava/lang/Object;)V");
            asm.areturn();
        },
    );

    // flatMap
    b.add_code(
        ACC_PUBLIC,
        "flatMap",
        "(Lscala/Function1;)Lscala/Option;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic("scala/None$", "MODULE$", "Lscala/None$;");
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.checkcast("scala/Option");
            asm.areturn();
        },
    );

    // foreach
    b.add_code(ACC_PUBLIC, "foreach", "(Lscala/Function1;)V", 2, |asm| {
        asm.aload(0);
        asm.invokevirtual("scala/Option", "isEmpty", "()Z");
        let end = asm.fresh_label();
        asm.ifne(end);
        asm.aload(1);
        asm.aload(0);
        asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
        asm.invokeinterface(
            "scala/Function1",
            "apply",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        );
        asm.pop();
        asm.mark(end);
        asm.vreturn();
    });

    // withFilter
    b.add_code(
        ACC_PUBLIC,
        "withFilter",
        "(Lscala/Function1;)Lscala/Option;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.aload(0);
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
            let keep = asm.fresh_label();
            asm.ifne(keep);
            asm.getstatic("scala/None$", "MODULE$", "Lscala/None$;");
            asm.areturn();
            asm.mark(keep);
            asm.aload(0);
            asm.areturn();
        },
    );

    add_option_predicates(&mut b);
    add_option_filters(&mut b);

    b.finish()
}

/// `isDefined` / `nonEmpty` / `getOrElse` / `contains` / `exists` / `forall`,
/// with the same erased descriptors the real `scala.Option` publishes.
fn add_option_predicates(b: &mut B) {
    for name in ["isDefined", "nonEmpty"] {
        b.add_code(ACC_PUBLIC, name, "()Z", 1, |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let empty = asm.fresh_label();
            asm.ifne(empty);
            asm.iconst(1);
            asm.ireturn();
            asm.mark(empty);
            asm.iconst(0);
            asm.ireturn();
        });
    }

    b.add_code(
        ACC_PUBLIC,
        "getOrElse",
        "(Lscala/Function0;)Ljava/lang/Object;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.aload(1);
            asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.areturn();
        },
    );

    // nsc: `!isEmpty && this.get == elem` — `==` is null-safe equality.
    b.add_code(ACC_PUBLIC, "contains", "(Ljava/lang/Object;)Z", 2, |asm| {
        asm.aload(0);
        asm.invokevirtual("scala/Option", "isEmpty", "()Z");
        let empty = asm.fresh_label();
        asm.ifne(empty);
        asm.aload(0);
        asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
        asm.aload(1);
        asm.invokestatic(
            "java/util/Objects",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        );
        asm.ireturn();
        asm.mark(empty);
        asm.iconst(0);
        asm.ireturn();
    });

    // `exists` is false on None, `forall` is true on None.
    for (name, on_empty) in [("exists", 0), ("forall", 1)] {
        b.add_code(ACC_PUBLIC, name, "(Lscala/Function1;)Z", 2, move |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let empty = asm.fresh_label();
            asm.ifne(empty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
            asm.ireturn();
            asm.mark(empty);
            asm.iconst(on_empty);
            asm.ireturn();
        });
    }
}

/// `filter` / `filterNot` / `orElse` / `fold`.
fn add_option_filters(b: &mut B) {
    // `filter` keeps `this` when the predicate holds, `filterNot` when it does
    // not; both keep `None` as is.
    for (name, keep_when) in [("filter", true), ("filterNot", false)] {
        b.add_code(
            ACC_PUBLIC,
            name,
            "(Lscala/Function1;)Lscala/Option;",
            2,
            move |asm| {
                asm.aload(0);
                asm.invokevirtual("scala/Option", "isEmpty", "()Z");
                let keep = asm.fresh_label();
                asm.ifne(keep);
                asm.aload(1);
                asm.aload(0);
                asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
                asm.invokeinterface(
                    "scala/Function1",
                    "apply",
                    "(Ljava/lang/Object;)Ljava/lang/Object;",
                );
                asm.checkcast("java/lang/Boolean");
                asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
                if keep_when {
                    asm.ifne(keep);
                } else {
                    asm.ifeq(keep);
                }
                asm.getstatic("scala/None$", "MODULE$", "Lscala/None$;");
                asm.areturn();
                asm.mark(keep);
                asm.aload(0);
                asm.areturn();
            },
        );
    }

    b.add_code(
        ACC_PUBLIC,
        "orElse",
        "(Lscala/Function0;)Lscala/Option;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let this = asm.fresh_label();
            asm.ifeq(this);
            asm.aload(1);
            asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
            asm.checkcast("scala/Option");
            asm.areturn();
            asm.mark(this);
            asm.aload(0);
            asm.areturn();
        },
    );

    // nsc: `def fold[B](ifEmpty: => B)(f: A => B): B`, one JVM method.
    b.add_code(
        ACC_PUBLIC,
        "fold",
        "(Lscala/Function0;Lscala/Function1;)Ljava/lang/Object;",
        3,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/Option", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.aload(1);
            asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(2);
            asm.aload(0);
            asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.areturn();
        },
    );
}

fn emit_some() -> EmittedClass {
    let mut b = B::class("scala/Some", "scala/Option");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "value".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.add_code(ACC_PUBLIC, "<init>", "(Ljava/lang/Object;)V", 2, |asm| {
        asm.aload(0);
        asm.invokespecial("scala/Option", "<init>", "()V");
        asm.aload(0);
        asm.aload(1);
        asm.putfield("scala/Some", "value", "Ljava/lang/Object;");
        asm.vreturn();
    });
    b.add_code(ACC_PUBLIC, "isEmpty", "()Z", 1, |asm| {
        asm.iconst(0);
        asm.ireturn();
    });
    b.add_code(ACC_PUBLIC, "get", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.getfield("scala/Some", "value", "Ljava/lang/Object;");
        asm.areturn();
    });
    b.finish()
}

fn emit_some_module() -> EmittedClass {
    let mut b = B::class("scala/Some$", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "MODULE$".into(),
        desc: "Lscala/Some$;".into(),
    });
    b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.aload(0);
        asm.putstatic("scala/Some$", "MODULE$", "Lscala/Some$;");
        asm.vreturn();
    });
    b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
        asm.new_obj("scala/Some$");
        asm.dup();
        asm.invokespecial("scala/Some$", "<init>", "()V");
        asm.pop();
        asm.vreturn();
    });
    b.add_code(
        ACC_PUBLIC,
        "apply",
        "(Ljava/lang/Object;)Lscala/Some;",
        2,
        |asm| {
            asm.new_obj("scala/Some");
            asm.dup();
            asm.aload(1);
            asm.invokespecial("scala/Some", "<init>", "(Ljava/lang/Object;)V");
            asm.areturn();
        },
    );
    b.finish()
}

fn emit_none() -> EmittedClass {
    let mut b = B::class("scala/None$", "scala/Option");
    b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "MODULE$".into(),
        desc: "Lscala/None$;".into(),
    });
    b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("scala/Option", "<init>", "()V");
        asm.aload(0);
        asm.putstatic("scala/None$", "MODULE$", "Lscala/None$;");
        asm.vreturn();
    });
    b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
        asm.new_obj("scala/None$");
        asm.dup();
        asm.invokespecial("scala/None$", "<init>", "()V");
        asm.pop();
        asm.vreturn();
    });
    b.add_code(ACC_PUBLIC, "isEmpty", "()Z", 1, |asm| {
        asm.iconst(1);
        asm.ireturn();
    });
    b.add_code(ACC_PUBLIC, "get", "()Ljava/lang/Object;", 1, |asm| {
        asm.new_obj("java/lang/RuntimeException");
        asm.dup();
        asm.ldc_string("None.get");
        asm.invokespecial(
            "java/lang/RuntimeException",
            "<init>",
            "(Ljava/lang/String;)V",
        );
        asm.athrow();
    });
    b.finish()
}

fn emit_list() -> EmittedClass {
    let mut b = B::class("scala/collection/immutable/List", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_ABSTRACT;
    b.add_code(ACC_PUBLIC, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.vreturn();
    });
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "isEmpty", "()Z");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "head", "()Ljava/lang/Object;");
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "tail",
        "()Lscala/collection/immutable/List;",
    );

    // prepend: this.::(x)  =>  new ::(x, this)
    b.add_code(
        ACC_PUBLIC,
        "::",
        "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.new_obj("scala/collection/immutable/$colon$colon");
            asm.dup();
            asm.aload(1);
            asm.aload(0);
            asm.invokespecial(
                "scala/collection/immutable/$colon$colon",
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "append",
        "(Lscala/collection/immutable/List;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.aload(1);
            asm.areturn();
            asm.mark(nonempty);
            asm.new_obj("scala/collection/immutable/$colon$colon");
            asm.dup();
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "head",
                "()Ljava/lang/Object;",
            );
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "tail",
                "()Lscala/collection/immutable/List;",
            );
            asm.aload(1);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "append",
                "(Lscala/collection/immutable/List;)Lscala/collection/immutable/List;",
            );
            asm.invokespecial(
                "scala/collection/immutable/$colon$colon",
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "map",
        "(Lscala/Function1;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic(
                "scala/collection/immutable/Nil$",
                "MODULE$",
                "Lscala/collection/immutable/Nil$;",
            );
            asm.areturn();
            asm.mark(nonempty);
            asm.new_obj("scala/collection/immutable/$colon$colon");
            asm.dup();
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "head",
                "()Ljava/lang/Object;",
            );
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "tail",
                "()Lscala/collection/immutable/List;",
            );
            asm.aload(1);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "map",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.invokespecial(
                "scala/collection/immutable/$colon$colon",
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "flatMap",
        "(Lscala/Function1;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic(
                "scala/collection/immutable/Nil$",
                "MODULE$",
                "Lscala/collection/immutable/Nil$;",
            );
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "head",
                "()Ljava/lang/Object;",
            );
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.checkcast("scala/collection/immutable/List");
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "tail",
                "()Lscala/collection/immutable/List;",
            );
            asm.aload(1);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "flatMap",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "append",
                "(Lscala/collection/immutable/List;)Lscala/collection/immutable/List;",
            );
            asm.areturn();
        },
    );

    b.add_code(ACC_PUBLIC, "foreach", "(Lscala/Function1;)V", 3, |asm| {
        asm.aload(0);
        asm.astore(2);
        let loop_l = asm.fresh_label();
        let end = asm.fresh_label();
        asm.mark(loop_l);
        asm.aload(2);
        asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
        asm.ifne(end);
        asm.aload(1);
        asm.aload(2);
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "head",
            "()Ljava/lang/Object;",
        );
        asm.invokeinterface(
            "scala/Function1",
            "apply",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        );
        asm.pop();
        asm.aload(2);
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "tail",
            "()Lscala/collection/immutable/List;",
        );
        asm.astore(2);
        asm.goto(loop_l);
        asm.mark(end);
        asm.vreturn();
    });

    b.add_code(
        ACC_PUBLIC,
        "withFilter",
        "(Lscala/Function1;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic(
                "scala/collection/immutable/Nil$",
                "MODULE$",
                "Lscala/collection/immutable/Nil$;",
            );
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "head",
                "()Ljava/lang/Object;",
            );
            asm.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
            let keep = asm.fresh_label();
            asm.ifne(keep);
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "tail",
                "()Lscala/collection/immutable/List;",
            );
            asm.aload(1);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "withFilter",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.areturn();
            asm.mark(keep);
            asm.new_obj("scala/collection/immutable/$colon$colon");
            asm.dup();
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "head",
                "()Ljava/lang/Object;",
            );
            asm.aload(0);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "tail",
                "()Lscala/collection/immutable/List;",
            );
            asm.aload(1);
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "withFilter",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.invokespecial(
                "scala/collection/immutable/$colon$colon",
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
        },
    );

    add_list_core_runtime(&mut b);

    b.finish()
}

/// Implementations for what `prelude_seq::add_list_core_private` declares.
///
/// The private runtime's `List` has only `isEmpty` / `head` / `tail` as abstract
/// methods, so everything here is built on top of those. scala-library is not linked.
fn add_list_core_runtime(b: &mut B) {
    const LIST: &str = "scala/collection/immutable/List";
    const CONS: &str = "scala/collection/immutable/$colon$colon";
    const NIL: &str = "scala/collection/immutable/Nil$";
    const F1: &str = "scala/Function1";

    // length: 0 if empty else 1 + tail.length
    b.add_code(ACC_PUBLIC, "length", "()I", 1, |asm| {
        asm.aload(0);
        asm.invokevirtual(LIST, "isEmpty", "()Z");
        let nonempty = asm.fresh_label();
        asm.ifeq(nonempty);
        asm.iconst(0);
        asm.ireturn();
        asm.mark(nonempty);
        asm.iconst(1);
        asm.aload(0);
        asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
        asm.invokevirtual(LIST, "length", "()I");
        asm.iadd();
        asm.ireturn();
    });

    b.add_code(ACC_PUBLIC, "size", "()I", 1, |asm| {
        asm.aload(0);
        asm.invokevirtual(LIST, "length", "()I");
        asm.ireturn();
    });

    b.add_code(ACC_PUBLIC, "nonEmpty", "()Z", 1, |asm| {
        asm.aload(0);
        asm.invokevirtual(LIST, "isEmpty", "()Z");
        let empty = asm.fresh_label();
        asm.ifne(empty);
        asm.iconst(1);
        asm.ireturn();
        asm.mark(empty);
        asm.iconst(0);
        asm.ireturn();
    });

    // last: tail.isEmpty ? head : tail.last
    b.add_code(ACC_PUBLIC, "last", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
        asm.invokevirtual(LIST, "isEmpty", "()Z");
        let more = asm.fresh_label();
        asm.ifeq(more);
        asm.aload(0);
        asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
        asm.areturn();
        asm.mark(more);
        asm.aload(0);
        asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
        asm.invokevirtual(LIST, "last", "()Ljava/lang/Object;");
        asm.areturn();
    });

    // reverse: iterative accumulate
    b.add_code(
        ACC_PUBLIC,
        "reverse",
        "()Lscala/collection/immutable/List;",
        3,
        |asm| {
            asm.getstatic(NIL, "MODULE$", "Lscala/collection/immutable/Nil$;");
            asm.checkcast(LIST);
            asm.astore(1);
            asm.aload(0);
            asm.astore(2);
            let loop_l = asm.fresh_label();
            let end = asm.fresh_label();
            asm.mark(loop_l);
            asm.aload(2);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            asm.ifne(end);
            asm.aload(1);
            asm.aload(2);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.invokevirtual(
                LIST,
                "::",
                "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
            );
            asm.astore(1);
            asm.aload(2);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.astore(2);
            asm.goto(loop_l);
            asm.mark(end);
            asm.aload(1);
            asm.areturn();
        },
    );

    // filter: the private runtime's `withFilter` is a strict filter already.
    b.add_code(
        ACC_PUBLIC,
        "filter",
        "(Lscala/Function1;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.aload(1);
            asm.invokevirtual(
                LIST,
                "withFilter",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "filterNot",
        "(Lscala/Function1;)Lscala/collection/immutable/List;",
        2,
        |asm| {
            asm.aload(0);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic(NIL, "MODULE$", "Lscala/collection/immutable/Nil$;");
            asm.areturn();
            asm.mark(nonempty);
            asm.aload(1);
            asm.aload(0);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.invokeinterface(F1, "apply", "(Ljava/lang/Object;)Ljava/lang/Object;");
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
            let drop = asm.fresh_label();
            asm.ifne(drop);
            asm.new_obj(CONS);
            asm.dup();
            asm.aload(0);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.aload(0);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.aload(1);
            asm.invokevirtual(
                LIST,
                "filterNot",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.invokespecial(
                CONS,
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
            asm.mark(drop);
            asm.aload(0);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.aload(1);
            asm.invokevirtual(
                LIST,
                "filterNot",
                "(Lscala/Function1;)Lscala/collection/immutable/List;",
            );
            asm.areturn();
        },
    );

    b.add_code(ACC_PUBLIC, "contains", "(Ljava/lang/Object;)Z", 3, |asm| {
        asm.aload(0);
        asm.astore(2);
        let loop_l = asm.fresh_label();
        let end = asm.fresh_label();
        let hit = asm.fresh_label();
        asm.mark(loop_l);
        asm.aload(2);
        asm.invokevirtual(LIST, "isEmpty", "()Z");
        asm.ifne(end);
        asm.aload(1);
        asm.aload(2);
        asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
        asm.invokestatic(
            "java/util/Objects",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        );
        asm.ifne(hit);
        asm.aload(2);
        asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
        asm.astore(2);
        asm.goto(loop_l);
        asm.mark(hit);
        asm.iconst(1);
        asm.ireturn();
        asm.mark(end);
        asm.iconst(0);
        asm.ireturn();
    });

    for (name, stop_on) in [("exists", true), ("forall", false)] {
        b.add_code(ACC_PUBLIC, name, "(Lscala/Function1;)Z", 3, move |asm| {
            asm.aload(0);
            asm.astore(2);
            let loop_l = asm.fresh_label();
            let end = asm.fresh_label();
            let hit = asm.fresh_label();
            asm.mark(loop_l);
            asm.aload(2);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            asm.ifne(end);
            asm.aload(1);
            asm.aload(2);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.invokeinterface(F1, "apply", "(Ljava/lang/Object;)Ljava/lang/Object;");
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
            if stop_on {
                asm.ifne(hit);
            } else {
                asm.ifeq(hit);
            }
            asm.aload(2);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.astore(2);
            asm.goto(loop_l);
            asm.mark(hit);
            asm.iconst(if stop_on { 1 } else { 0 });
            asm.ireturn();
            asm.mark(end);
            asm.iconst(if stop_on { 0 } else { 1 });
            asm.ireturn();
        });
    }

    b.add_code(ACC_PUBLIC, "count", "(Lscala/Function1;)I", 4, |asm| {
        asm.iconst(0);
        asm.istore(2);
        asm.aload(0);
        asm.astore(3);
        let loop_l = asm.fresh_label();
        let end = asm.fresh_label();
        let skip = asm.fresh_label();
        asm.mark(loop_l);
        asm.aload(3);
        asm.invokevirtual(LIST, "isEmpty", "()Z");
        asm.ifne(end);
        asm.aload(1);
        asm.aload(3);
        asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
        asm.invokeinterface(F1, "apply", "(Ljava/lang/Object;)Ljava/lang/Object;");
        asm.checkcast("java/lang/Boolean");
        asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
        asm.ifeq(skip);
        asm.iload(2);
        asm.iconst(1);
        asm.iadd();
        asm.istore(2);
        asm.mark(skip);
        asm.aload(3);
        asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
        asm.astore(3);
        asm.goto(loop_l);
        asm.mark(end);
        asm.iload(2);
        asm.ireturn();
    });

    b.add_code(
        ACC_PUBLIC,
        "take",
        "(I)Lscala/collection/immutable/List;",
        2,
        |asm| {
            let cont = asm.fresh_label();
            asm.iload(1);
            asm.ifgt(cont);
            asm.getstatic(NIL, "MODULE$", "Lscala/collection/immutable/Nil$;");
            asm.areturn();
            asm.mark(cont);
            asm.aload(0);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            let nonempty = asm.fresh_label();
            asm.ifeq(nonempty);
            asm.getstatic(NIL, "MODULE$", "Lscala/collection/immutable/Nil$;");
            asm.areturn();
            asm.mark(nonempty);
            asm.new_obj(CONS);
            asm.dup();
            asm.aload(0);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.aload(0);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.iload(1);
            asm.iconst(1);
            asm.isub();
            asm.invokevirtual(LIST, "take", "(I)Lscala/collection/immutable/List;");
            asm.invokespecial(
                CONS,
                "<init>",
                "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "drop",
        "(I)Lscala/collection/immutable/List;",
        3,
        |asm| {
            asm.aload(0);
            asm.astore(2);
            let loop_l = asm.fresh_label();
            let end = asm.fresh_label();
            asm.mark(loop_l);
            asm.iload(1);
            asm.ifle(end);
            asm.aload(2);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            asm.ifne(end);
            asm.aload(2);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.astore(2);
            asm.iload(1);
            asm.iconst(1);
            asm.isub();
            asm.istore(1);
            asm.goto(loop_l);
            asm.mark(end);
            asm.aload(2);
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "mkString",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        7,
        |asm| {
            asm.new_obj("java/lang/StringBuilder");
            asm.dup();
            asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
            asm.astore(4);
            asm.aload(4);
            asm.aload(1);
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.pop();
            asm.aload(0);
            asm.astore(5);
            asm.iconst(1);
            asm.istore(6);
            let loop_l = asm.fresh_label();
            let end = asm.fresh_label();
            let no_sep = asm.fresh_label();
            asm.mark(loop_l);
            asm.aload(5);
            asm.invokevirtual(LIST, "isEmpty", "()Z");
            asm.ifne(end);
            asm.iload(6);
            asm.ifne(no_sep);
            asm.aload(4);
            asm.aload(2);
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.pop();
            asm.mark(no_sep);
            asm.iconst(0);
            asm.istore(6);
            asm.aload(4);
            asm.aload(5);
            asm.invokevirtual(LIST, "head", "()Ljava/lang/Object;");
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
            );
            asm.pop();
            asm.aload(5);
            asm.invokevirtual(LIST, "tail", "()Lscala/collection/immutable/List;");
            asm.astore(5);
            asm.goto(loop_l);
            asm.mark(end);
            asm.aload(4);
            asm.aload(3);
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.pop();
            asm.aload(4);
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "toString",
                "()Ljava/lang/String;",
            );
            asm.areturn();
        },
    );

    b.add_code(
        ACC_PUBLIC,
        "mkString",
        "(Ljava/lang/String;)Ljava/lang/String;",
        2,
        |asm| {
            asm.aload(0);
            asm.ldc_string("");
            asm.aload(1);
            asm.ldc_string("");
            asm.invokevirtual(
                LIST,
                "mkString",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            );
            asm.areturn();
        },
    );

    b.add_code(ACC_PUBLIC, "mkString", "()Ljava/lang/String;", 1, |asm| {
        asm.aload(0);
        asm.ldc_string("");
        asm.invokevirtual(LIST, "mkString", "(Ljava/lang/String;)Ljava/lang/String;");
        asm.areturn();
    });
}

fn emit_cons() -> EmittedClass {
    let mut b = B::class(
        "scala/collection/immutable/$colon$colon",
        "scala/collection/immutable/List",
    );
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "head".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "tl".into(),
        desc: "Lscala/collection/immutable/List;".into(),
    });
    b.add_code(
        ACC_PUBLIC,
        "<init>",
        "(Ljava/lang/Object;Lscala/collection/immutable/List;)V",
        3,
        |asm| {
            asm.aload(0);
            asm.invokespecial("scala/collection/immutable/List", "<init>", "()V");
            asm.aload(0);
            asm.aload(1);
            asm.putfield(
                "scala/collection/immutable/$colon$colon",
                "head",
                "Ljava/lang/Object;",
            );
            asm.aload(0);
            asm.aload(2);
            asm.putfield(
                "scala/collection/immutable/$colon$colon",
                "tl",
                "Lscala/collection/immutable/List;",
            );
            asm.vreturn();
        },
    );
    b.add_code(ACC_PUBLIC, "isEmpty", "()Z", 1, |asm| {
        asm.iconst(0);
        asm.ireturn();
    });
    b.add_code(ACC_PUBLIC, "head", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.getfield(
            "scala/collection/immutable/$colon$colon",
            "head",
            "Ljava/lang/Object;",
        );
        asm.areturn();
    });
    b.add_code(
        ACC_PUBLIC,
        "tail",
        "()Lscala/collection/immutable/List;",
        1,
        |asm| {
            asm.aload(0);
            asm.getfield(
                "scala/collection/immutable/$colon$colon",
                "tl",
                "Lscala/collection/immutable/List;",
            );
            asm.areturn();
        },
    );
    b.finish()
}

fn emit_nil() -> EmittedClass {
    let mut b = B::class(
        "scala/collection/immutable/Nil$",
        "scala/collection/immutable/List",
    );
    b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "MODULE$".into(),
        desc: "Lscala/collection/immutable/Nil$;".into(),
    });
    b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("scala/collection/immutable/List", "<init>", "()V");
        asm.aload(0);
        asm.putstatic(
            "scala/collection/immutable/Nil$",
            "MODULE$",
            "Lscala/collection/immutable/Nil$;",
        );
        asm.vreturn();
    });
    b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
        asm.new_obj("scala/collection/immutable/Nil$");
        asm.dup();
        asm.invokespecial("scala/collection/immutable/Nil$", "<init>", "()V");
        asm.pop();
        asm.vreturn();
    });
    b.add_code(ACC_PUBLIC, "isEmpty", "()Z", 1, |asm| {
        asm.iconst(1);
        asm.ireturn();
    });
    b.add_code(ACC_PUBLIC, "head", "()Ljava/lang/Object;", 1, |asm| {
        asm.new_obj("java/lang/RuntimeException");
        asm.dup();
        asm.ldc_string("Nil.head");
        asm.invokespecial(
            "java/lang/RuntimeException",
            "<init>",
            "(Ljava/lang/String;)V",
        );
        asm.athrow();
    });
    b.add_code(
        ACC_PUBLIC,
        "tail",
        "()Lscala/collection/immutable/List;",
        1,
        |asm| {
            asm.new_obj("java/lang/RuntimeException");
            asm.dup();
            asm.ldc_string("Nil.tail");
            asm.invokespecial(
                "java/lang/RuntimeException",
                "<init>",
                "(Ljava/lang/String;)V",
            );
            asm.athrow();
        },
    );
    b.finish()
}

fn emit_list_module() -> EmittedClass {
    let mut b = B::class("scala/collection/immutable/List$", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "MODULE$".into(),
        desc: "Lscala/collection/immutable/List$;".into(),
    });
    b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.aload(0);
        asm.putstatic(
            "scala/collection/immutable/List$",
            "MODULE$",
            "Lscala/collection/immutable/List$;",
        );
        asm.vreturn();
    });
    b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
        asm.new_obj("scala/collection/immutable/List$");
        asm.dup();
        asm.invokespecial("scala/collection/immutable/List$", "<init>", "()V");
        asm.pop();
        asm.vreturn();
    });
    b.add_code(
        ACC_PUBLIC,
        "unapplySeq",
        "(Lscala/collection/immutable/List;)Lscala/Option;",
        2,
        |asm| {
            asm.new_obj("scala/Some");
            asm.dup();
            asm.aload(1);
            asm.invokespecial("scala/Some", "<init>", "(Ljava/lang/Object;)V");
            asm.areturn();
        },
    );
    b.finish()
}

fn emit_tuple2() -> EmittedClass {
    let mut b = B::class("scala/Tuple2", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_FINAL,
        name: "_1".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_FINAL,
        name: "_2".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.add_code(
        ACC_PUBLIC,
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        3,
        |asm| {
            asm.aload(0);
            asm.invokespecial("java/lang/Object", "<init>", "()V");
            asm.aload(0);
            asm.aload(1);
            asm.putfield("scala/Tuple2", "_1", "Ljava/lang/Object;");
            asm.aload(0);
            asm.aload(2);
            asm.putfield("scala/Tuple2", "_2", "Ljava/lang/Object;");
            asm.vreturn();
        },
    );
    b.finish()
}

fn emit_dynamic() -> EmittedClass {
    // Marker interface, same role as scala-library `scala.Dynamic`.
    let mut b = B::class("scala/Dynamic", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces.clear();
    b.finish()
}

fn emit_arrow_assoc() -> EmittedClass {
    let mut b = B::class("scala/runtime/ArrowAssoc", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_FINAL,
        name: "self".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.add_code(ACC_PUBLIC, "<init>", "(Ljava/lang/Object;)V", 2, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.aload(0);
        asm.aload(1);
        asm.putfield("scala/runtime/ArrowAssoc", "self", "Ljava/lang/Object;");
        asm.vreturn();
    });
    b.add_code(
        ACC_PUBLIC,
        "->",
        "(Ljava/lang/Object;)Lscala/Tuple2;",
        2,
        |asm| {
            asm.new_obj("scala/Tuple2");
            asm.dup();
            asm.aload(0);
            asm.getfield("scala/runtime/ArrowAssoc", "self", "Ljava/lang/Object;");
            asm.aload(1);
            asm.invokespecial(
                "scala/Tuple2",
                "<init>",
                "(Ljava/lang/Object;Ljava/lang/Object;)V",
            );
            asm.areturn();
        },
    );
    b.finish()
}

/// One `scala.runtime.<name>` box: a mutable `elem` field, a constructor and
/// the static `create` factory the backend calls when boxing a captured `var`.
fn emit_ref_box(name: &str, elem: &str) -> EmittedClass {
    let internal = format!("scala/runtime/{name}");
    let mut b = B::class(&internal, "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_FINAL;
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "elem".into(),
        desc: elem.into(),
    });
    let wide = matches!(elem, "J" | "D");
    let slots = if wide { 3 } else { 2 };
    let load_arg = |asm: &mut Assembler, n: u16| match elem {
        "J" => asm.lload(n),
        "D" => asm.dload(n),
        "F" => asm.fload(n),
        "Ljava/lang/Object;" => asm.aload(n),
        _ => asm.iload(n),
    };
    {
        let internal = internal.clone();
        b.add_code(
            ACC_PUBLIC,
            "<init>",
            &format!("({elem})V"),
            slots,
            move |asm| {
                asm.aload(0);
                asm.invokespecial("java/lang/Object", "<init>", "()V");
                asm.aload(0);
                load_arg(asm, 1);
                asm.putfield(&internal, "elem", elem);
                asm.vreturn();
            },
        );
    }
    {
        let internal = internal.clone();
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            "create",
            &format!("({elem})L{internal};"),
            slots - 1,
            move |asm| {
                asm.new_obj(&internal);
                asm.dup();
                load_arg(asm, 0);
                asm.invokespecial(&internal, "<init>", &format!("({elem})V"));
                asm.areturn();
            },
        );
    }
    b.finish()
}

/// Private-runtime stand-in for `scala.runtime.LazyRef` and its unboxed
/// siblings. Mirrors scala-library: a `@volatile` `_initialized` flag written
/// *after* `_value`, so a reader that sees the flag also sees the value, and an
/// initialiser that throws leaves the cell untouched for the next attempt.
fn emit_lazy_cell(name: &str, elem: &str) -> EmittedClass {
    let internal = format!("scala/runtime/{name}");
    let mut b = B::class(&internal, "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.interfaces.push("java/io/Serializable".into());
    b.fields.push(Field {
        access: ACC_PRIVATE | ACC_VOLATILE,
        name: "_initialized".into(),
        desc: "Z".into(),
    });
    if !elem.is_empty() {
        b.fields.push(Field {
            access: ACC_PRIVATE,
            name: "_value".into(),
            desc: elem.into(),
        });
    }
    {
        let internal = internal.clone();
        b.add_code(ACC_PUBLIC, "<init>", "()V", 1, move |asm| {
            asm.aload(0);
            asm.invokespecial("java/lang/Object", "<init>", "()V");
            let _ = &internal;
            asm.vreturn();
        });
    }
    {
        let internal = internal.clone();
        b.add_code(ACC_PUBLIC, "initialized", "()Z", 1, move |asm| {
            asm.aload(0);
            asm.getfield(&internal, "_initialized", "Z");
            asm.ireturn();
        });
    }
    if elem.is_empty() {
        let internal = internal.clone();
        b.add_code(ACC_PUBLIC, "initialize", "()V", 1, move |asm| {
            asm.aload(0);
            asm.iconst(1);
            asm.putfield(&internal, "_initialized", "Z");
            asm.vreturn();
        });
        return b.finish();
    }
    let wide = matches!(elem, "J" | "D");
    let load_arg = |asm: &mut Assembler, n: u16| match elem {
        "J" => asm.lload(n),
        "D" => asm.dload(n),
        "F" => asm.fload(n),
        "Ljava/lang/Object;" => asm.aload(n),
        _ => asm.iload(n),
    };
    let ret_val = |asm: &mut Assembler| match elem {
        "J" => asm.lreturn(),
        "D" => asm.dreturn(),
        "F" => asm.freturn(),
        "Ljava/lang/Object;" => asm.areturn(),
        _ => asm.ireturn(),
    };
    {
        let internal = internal.clone();
        b.add_code(ACC_PUBLIC, "value", &format!("(){elem}"), 1, move |asm| {
            asm.aload(0);
            asm.getfield(&internal, "_value", elem);
            ret_val(asm);
        });
    }
    {
        let internal = internal.clone();
        b.add_code(
            ACC_PUBLIC,
            "initialize",
            &format!("({elem}){elem}"),
            if wide { 3 } else { 2 },
            move |asm| {
                asm.aload(0);
                load_arg(asm, 1);
                asm.putfield(&internal, "_value", elem);
                asm.aload(0);
                asm.iconst(1);
                asm.putfield(&internal, "_initialized", "Z");
                load_arg(asm, 1);
                ret_val(asm);
            },
        );
    }
    b.finish()
}

/// Private-runtime stand-in for `scala.runtime.BoxedUnit`. `Unit` erases to
/// `V` only as a method result; as a parameter, a field, an array element or a
/// type argument it erases to this class, and the single value `()` is the
/// `UNIT` singleton. Without it the private runtime had to box `()` as `null`,
/// so `(x: Any) => println(x)` printed `null` where scalac prints `()` and a
/// `case () =>` pattern also matched `null`.
///
/// Mirrors scala-library's shape: `private` constructor, `UNIT`, `TYPE`,
/// `equals` by identity, `hashCode` 0, `toString` `"()"`.
fn emit_boxed_unit() -> EmittedClass {
    const CN: &str = "scala/runtime/BoxedUnit";
    const CD: &str = "Lscala/runtime/BoxedUnit;";
    let mut b = B::class(CN, "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_FINAL;
    b.interfaces.push("java/io/Serializable".into());
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "UNIT".into(),
        desc: CD.into(),
    });
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
        name: "TYPE".into(),
        desc: "Ljava/lang/Class;".into(),
    });
    b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Object", "<init>", "()V");
        asm.vreturn();
    });
    b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
        asm.new_obj(CN);
        asm.dup();
        asm.invokespecial(CN, "<init>", "()V");
        asm.putstatic(CN, "UNIT", CD);
        asm.getstatic("java/lang/Void", "TYPE", "Ljava/lang/Class;");
        asm.putstatic(CN, "TYPE", "Ljava/lang/Class;");
        asm.vreturn();
    });
    b.add_code(
        ACC_PUBLIC,
        "equals",
        "(Ljava/lang/Object;)Z",
        2,
        |asm: &mut Assembler| {
            let ne = asm.fresh_label();
            asm.aload(0);
            asm.aload(1);
            asm.if_acmpne(ne);
            asm.iconst(1);
            asm.ireturn();
            asm.mark(ne);
            asm.iconst(0);
            asm.ireturn();
        },
    );
    b.add_code(ACC_PUBLIC, "hashCode", "()I", 1, |asm| {
        asm.iconst(0);
        asm.ireturn();
    });
    b.add_code(ACC_PUBLIC, "toString", "()Ljava/lang/String;", 1, |asm| {
        asm.ldc_string("()");
        asm.areturn();
    });
    b.finish()
}

/// Private-runtime stand-in for `scala.runtime.Nothing$`. `Nothing` erases to
/// `V` as a method result but to this class in a parameter, so
/// `def f(x: Nothing)` is `(Lscala/runtime/Nothing$;)I` — and the verifier
/// loads a parameter's class even for a method nobody can call. Mirrors
/// scala-library: `public abstract class Nothing$ extends Throwable`.
fn emit_nothing() -> EmittedClass {
    let mut b = B::class("scala/runtime/Nothing$", "java/lang/Throwable");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_ABSTRACT;
    b.add_code(ACC_PUBLIC, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.invokespecial("java/lang/Throwable", "<init>", "()V");
        asm.vreturn();
    });
    b.finish()
}

fn emit_not_implemented() -> EmittedClass {
    let mut b = B::class("scala/NotImplementedError", "java/lang/RuntimeException");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.add_code(ACC_PUBLIC, "<init>", "()V", 1, |asm| {
        asm.aload(0);
        asm.ldc_string("an implementation is missing");
        asm.invokespecial(
            "java/lang/RuntimeException",
            "<init>",
            "(Ljava/lang/String;)V",
        );
        asm.vreturn();
    });
    b.add_code(ACC_PUBLIC, "<init>", "(Ljava/lang/String;)V", 2, |asm| {
        asm.aload(0);
        asm.aload(1);
        asm.invokespecial(
            "java/lang/RuntimeException",
            "<init>",
            "(Ljava/lang/String;)V",
        );
        asm.vreturn();
    });
    b.finish()
}

/// Private-runtime `scala.MatchError`: a `match` that runs out of cases throws
/// this in both modes, so the class user code catches and the message it prints
/// are the same with and without the jar. 2.13's message is
/// `"<obj> (of class <class name>)"`, or `"null"`.
fn emit_match_error() -> EmittedClass {
    let mut b = B::class("scala/MatchError", "java/lang/RuntimeException");
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC | ACC_FINAL,
        name: "obj".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    // The message is built by a static helper so the constructor has no branch
    // while `this` is still uninitialised.
    b.add_code(
        ACC_PRIVATE | ACC_STATIC,
        "objString",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        2,
        |asm| {
            asm.aload(0);
            let is_null = asm.fresh_label();
            asm.ifnull(is_null);
            asm.new_obj("java/lang/StringBuilder");
            asm.dup();
            asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
            asm.aload(0);
            asm.invokestatic(
                "java/lang/String",
                "valueOf",
                "(Ljava/lang/Object;)Ljava/lang/String;",
            );
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.ldc_string(" (of class ");
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.aload(0);
            asm.invokevirtual("java/lang/Object", "getClass", "()Ljava/lang/Class;");
            asm.invokevirtual("java/lang/Class", "getName", "()Ljava/lang/String;");
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.ldc_string(")");
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            );
            asm.invokevirtual(
                "java/lang/StringBuilder",
                "toString",
                "()Ljava/lang/String;",
            );
            asm.areturn();
            asm.mark(is_null);
            asm.ldc_string("null");
            asm.areturn();
        },
    );
    b.add_code(ACC_PUBLIC, "<init>", "(Ljava/lang/Object;)V", 2, |asm| {
        asm.aload(0);
        asm.aload(1);
        asm.invokestatic(
            "scala/MatchError",
            "objString",
            "(Ljava/lang/Object;)Ljava/lang/String;",
        );
        asm.invokespecial(
            "java/lang/RuntimeException",
            "<init>",
            "(Ljava/lang/String;)V",
        );
        asm.aload(0);
        asm.aload(1);
        asm.putfield("scala/MatchError", "obj", "Ljava/lang/Object;");
        asm.vreturn();
    });
    b.add_code(ACC_PUBLIC, "obj", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.getfield("scala/MatchError", "obj", "Ljava/lang/Object;");
        asm.areturn();
    });
    b.finish()
}

/// Private-runtime stand-in for `scala.runtime.NonLocalReturnControl`.
/// Descriptor matches scala-library 2.13: `(Ljava/lang/Object;Ljava/lang/Object;)V`,
/// `key()` / `value()`.
fn emit_non_local_return_control() -> EmittedClass {
    let mut b = B::class(
        "scala/runtime/NonLocalReturnControl",
        "java/lang/RuntimeException",
    );
    b.access = ACC_PUBLIC | ACC_SUPER;
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "key".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.fields.push(Field {
        access: ACC_PUBLIC,
        name: "value".into(),
        desc: "Ljava/lang/Object;".into(),
    });
    b.add_code(
        ACC_PUBLIC,
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        3,
        |asm| {
            asm.aload(0);
            asm.invokespecial("java/lang/RuntimeException", "<init>", "()V");
            asm.aload(0);
            asm.aload(1);
            asm.putfield(
                "scala/runtime/NonLocalReturnControl",
                "key",
                "Ljava/lang/Object;",
            );
            asm.aload(0);
            asm.aload(2);
            asm.putfield(
                "scala/runtime/NonLocalReturnControl",
                "value",
                "Ljava/lang/Object;",
            );
            asm.vreturn();
        },
    );
    b.add_code(ACC_PUBLIC, "key", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.getfield(
            "scala/runtime/NonLocalReturnControl",
            "key",
            "Ljava/lang/Object;",
        );
        asm.areturn();
    });
    b.add_code(ACC_PUBLIC, "value", "()Ljava/lang/Object;", 1, |asm| {
        asm.aload(0);
        asm.getfield(
            "scala/runtime/NonLocalReturnControl",
            "value",
            "Ljava/lang/Object;",
        );
        asm.areturn();
    });
    b.finish()
}

fn emit_delayed_init() -> EmittedClass {
    let mut b = B::class("scala/DelayedInit", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces.clear();
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "delayedInit",
        "(Lscala/Function0;)V",
    );
    b.finish()
}

fn emit_app() -> EmittedClass {
    let mut b = B::class("scala/App", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces = vec!["scala/DelayedInit".into()];
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "delayedInit",
        "(Lscala/Function0;)V",
    );
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "main", "([Ljava/lang/String;)V");
    b.finish()
}
