//! JVM classfile emitter.

pub mod classfile;
pub mod code;
pub mod gen;
pub mod load;
pub mod pickle;
pub mod pickle_read;
pub mod runtime;

pub use classfile::EmittedClass;
pub use gen::{emit, emit_opts, EmitOpts};
pub use load::{load_classpath, scala_signature_bytes, LoadedClass, LoadedMethod};
pub use runtime::emit_runtime;
