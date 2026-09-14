//! JVM classfile emitter.

pub mod classfile;
pub mod code;
mod companion_fwd;
pub mod gen;
mod gen_call;
mod gen_class;
mod gen_desc;
mod gen_expr;
mod gen_invoke;
mod gen_lambda;
mod gen_match;
mod gen_object;
mod gen_tailrec;
mod gen_trait;
pub mod ifacebridge;
pub mod load;
pub mod pickle;
pub mod runtime;
pub mod sig;

pub use classfile::EmittedClass;
pub use gen::{emit, emit_opts, AbiMode, EmitError, EmitOpts, EmitResult};
pub use ifacebridge::BinaryParents;
pub use load::{
    load_classpath, load_classpath_checked, parse_abi_class, scala_signature_bytes,
    scala_signature_bytes_result, AbiClassError, AbiLoadedClass, AbiLoadedField, AbiLoadedMethod,
    ClassfileError, ClasspathLoadError, LoadedClass, LoadedField, LoadedMethod,
};
pub use pickle::{PickledType, PickledTypeParam};
pub use runtime::emit_runtime;
pub use sig::{record_generic_signatures, GenericSignature, GenericSignatures};
