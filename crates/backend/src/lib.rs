//! JVM classfile emitter.

pub mod classfile;
pub mod code;
pub mod gen;

pub use gen::{emit, EmittedClass};
