//! Adapter from canonical low-level ABI metadata to the typer's compatibility DTOs.

use crate::check::{
    ClasspathClass, ClasspathField, ClasspathMethod, ClasspathPickleMethod, ClasspathType,
    ClasspathTypeParam,
};

/// Consume classfile/ScalaSignature metadata without cloning its recursive
/// names and type trees in the driver.
pub fn adapt_classpath(classes: Vec<scala_rs_pickle::LoadedClass>) -> Vec<ClasspathClass> {
    classes.into_iter().map(adapt_class).collect()
}

fn adapt_class(c: scala_rs_pickle::LoadedClass) -> ClasspathClass {
    let (pickle, pickle_tparams, extends_anyval) = match c.pickle {
        Some(p) => {
            let scala_rs_pickle::PickledClass {
                tparams,
                methods,
                extends_anyval,
                ..
            } = p;
            (
                Some(methods.into_iter().map(adapt_pickle_method).collect()),
                tparams.into_iter().map(adapt_tparam).collect(),
                extends_anyval,
            )
        }
        None => (None, Vec::new(), false),
    };
    ClasspathClass {
        jvm_name: c.internal_name.into_string(),
        is_module: c.is_module,
        methods: c
            .methods
            .into_iter()
            .map(|m| ClasspathMethod {
                access: m.flags.bits(),
                name: m.name,
                desc: m.descriptor.into_string(),
                signature: m.signature,
            })
            .collect(),
        fields: c
            .fields
            .into_iter()
            .map(|f| ClasspathField {
                access: f.flags.bits(),
                name: f.name,
                desc: f.descriptor.into_string(),
            })
            .collect(),
        pickle,
        pickle_tparams,
        is_interface: c.flags.is_interface(),
        super_name: c.super_name.map(|n| n.into_string()),
        interfaces: c.interfaces.into_iter().map(|n| n.into_string()).collect(),
        extends_anyval,
    }
}

fn adapt_type(t: scala_rs_pickle::PickledType) -> ClasspathType {
    ClasspathType {
        name: t.name,
        args: t.args.into_iter().map(adapt_type).collect(),
    }
}

fn adapt_tparam(t: scala_rs_pickle::PickledTypeParam) -> ClasspathTypeParam {
    ClasspathTypeParam {
        name: t.name,
        tparams: t.tparams.into_iter().map(adapt_tparam).collect(),
    }
}

fn adapt_pickle_method(m: scala_rs_pickle::PickledMethod) -> ClasspathPickleMethod {
    ClasspathPickleMethod {
        name: m.name,
        param_names: m.param_names,
        param_types: m.param_types.into_iter().map(adapt_type).collect(),
        clause_sizes: m.clause_sizes,
        param_flags: m.param_flags,
        ret: adapt_type(m.ret),
        tparams: m.tparams.into_iter().map(adapt_tparam).collect(),
        is_val: m.is_val,
        is_ctor: m.is_ctor,
        is_implicit: m.is_implicit,
        is_deferred: m.is_deferred,
        is_mutable: m.is_mutable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scala_rs_pickle::{
        JvmClassFlags, JvmDescriptor, JvmInternalName, JvmMemberFlags, JvmMemberKind, LoadedClass,
        LoadedMethod, PickledClass, PickledMethod, PickledType,
    };

    #[test]
    fn adapter_preserves_recursive_pickle_shape_and_raw_method_flags() {
        let raw_flags = 0x0001 | 0x0040 | 0x1000;
        let classes = adapt_classpath(vec![LoadedClass {
            internal_name: JvmInternalName::new("example/C").unwrap(),
            flags: JvmClassFlags::new(0x0200),
            is_module: false,
            methods: vec![LoadedMethod {
                flags: JvmMemberFlags::new(JvmMemberKind::Method, raw_flags),
                name: "id".into(),
                descriptor: JvmDescriptor::method("(Ljava/lang/Object;)Ljava/lang/Object;")
                    .unwrap(),
                signature: Some("<A:Ljava/lang/Object;>(TA;)TA;".into()),
            }],
            fields: Vec::new(),
            pickle: Some(PickledClass {
                name: "C".into(),
                is_module: false,
                tparams: Vec::new(),
                methods: vec![PickledMethod {
                    name: "id".into(),
                    param_names: vec!["value".into()],
                    param_types: vec![PickledType {
                        name: "F".into(),
                        args: vec![PickledType::simple("A")],
                    }],
                    clause_sizes: vec![1],
                    param_flags: vec![1 << 9],
                    ret: PickledType::simple("A"),
                    tparams: Vec::new(),
                    is_val: false,
                    is_ctor: false,
                    is_implicit: true,
                    is_deferred: true,
                    is_mutable: false,
                }],
                extends_anyval: true,
            }),
            super_name: Some(JvmInternalName::new("java/lang/Object").unwrap()),
            interfaces: Vec::new(),
        }]);

        let class = &classes[0];
        assert_eq!(class.jvm_name, "example/C");
        assert!(class.is_interface);
        assert!(class.extends_anyval);
        assert_eq!(class.methods[0].access, raw_flags);
        assert_eq!(
            class.methods[0].desc,
            "(Ljava/lang/Object;)Ljava/lang/Object;"
        );
        let method = &class.pickle.as_ref().unwrap()[0];
        assert_eq!(method.param_types[0].name, "F");
        assert_eq!(method.param_types[0].args[0].name, "A");
        assert_eq!(method.param_flags, vec![1 << 9]);
        assert!(method.is_implicit);
        assert!(method.is_deferred);
    }
}
