//! Minimal scala-rs runtime classfiles (Java 8 / major 52).
//!
//! These are **not** scala-library. They exist so Option / List / FunctionN
//! from the prelude have JVM types that `scala-rs run` can load.

use crate::classfile::EmittedClass;
use crate::classfile::{
    encode_method_name, ClassEmit, Field, Method, Pool, ACC_ABSTRACT, ACC_FINAL, ACC_INTERFACE,
    ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
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

pub fn emit_runtime() -> Vec<EmittedClass> {
    let mut out = vec![
        emit_function_n(0),
        emit_function_n(1),
        emit_partial_function(),
        emit_ordered(),
        emit_ordered_class(),
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
        emit_not_implemented(),
        emit_non_local_return_control(),
        emit_delayed_init(),
        emit_app(),
    ];
    out.extend(REF_BOXES.iter().map(|(n, d)| emit_ref_box(n, d)));
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

fn emit_ordered() -> EmittedClass {
    let mut b = B::class("scala/math/Ordered", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
    b.interfaces.clear();
    b.add_abstract(
        ACC_PUBLIC | ACC_ABSTRACT,
        "compare",
        "(Ljava/lang/Object;)I",
    );
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "<", "(Ljava/lang/Object;)Z");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, ">", "(Ljava/lang/Object;)Z");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, "<=", "(Ljava/lang/Object;)Z");
    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, ">=", "(Ljava/lang/Object;)Z");
    b.finish()
}

fn emit_ordered_class() -> EmittedClass {
    let mut b = B::class("scala/math/Ordered$class", "java/lang/Object");
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_FINAL;
    let static_desc = "(Lscala/math/Ordered;Ljava/lang/Object;)Z";
    fn cmp_op(b: &mut B, name: &str, jump_true: impl Fn(&mut Assembler, crate::code::Label)) {
        let static_desc = "(Lscala/math/Ordered;Ljava/lang/Object;)Z";
        b.add_code(ACC_PUBLIC | ACC_STATIC, name, static_desc, 2, |asm| {
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
        let _ = static_desc;
    }
    cmp_op(&mut b, "<", |asm, t| asm.if_icmplt(t));
    cmp_op(&mut b, ">", |asm, t| asm.if_icmpgt(t));
    cmp_op(&mut b, "<=", |asm, t| asm.if_icmple(t));
    cmp_op(&mut b, ">=", |asm, t| asm.if_icmpge(t));
    let _ = static_desc;
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

/// `prelude_seq::add_list_core_private` が宣言する分の実装。
///
/// 私有ランタイムの `List` は `isEmpty` / `head` / `tail` だけを抽象メソッドに
///持つので、ここは全部それらの上に組み立てる。scala-library はリンクしない。
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
