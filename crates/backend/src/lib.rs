//! JVM classfile emitter.

pub mod classfile;
pub mod code;
pub mod gen;
pub mod runtime;

pub use classfile::EmittedClass;
pub use gen::{emit, emit_opts, EmitOpts};
pub use runtime::emit_runtime;
