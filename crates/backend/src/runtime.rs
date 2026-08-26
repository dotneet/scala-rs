//! Minimal scala-rs runtime classfiles (Java 6 / major 50).
//!
//! These are **not** scala-library. They exist so Option / List / FunctionN
//! from the prelude have JVM types that `scala-rs run` can load.

use crate::classfile::EmittedClass;
use crate::classfile::{
    ClassEmit, Field, Method, Pool, ACC_ABSTRACT, ACC_FINAL, ACC_INTERFACE, ACC_PRIVATE,
    ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
};
use crate::code::Assembler;

const SRC: &str = "runtime.scala";

pub fn emit_runtime() -> Vec<EmittedClass> {
    vec![
        emit_function_n(0),
        emit_function_n(1),
        emit_option(),
        emit_some(),
        emit_some_module(),
        emit_none(),
        emit_list(),
        emit_cons(),
        emit_nil(),
        emit_tuple2(),
        emit_arrow_assoc(),
        emit_not_implemented(),
    ]
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
        gen(&mut asm);
        let (code, pool) = asm.finish();
        self.pool = pool;
        self.methods.push(Method {
            access,
            name: name.to_string(),
            desc: desc.to_string(),
            code: Some(code),
        });
    }

    fn add_abstract(&mut self, access: u16, name: &str, desc: &str) {
        self.methods.push(Method {
            access,
            name: name.to_string(),
            desc: desc.to_string(),
            code: None,
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

    b.finish()
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

    b.finish()
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
