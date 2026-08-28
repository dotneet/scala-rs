//! Reader for the nsc 2.13 `ScalaSignature` pickle format.
//!
//! Split out of `scala-rs-backend` so `scala-rs-typer` can use it too: the
//! typer completes library members on demand from these signatures, and it
//! cannot depend on the backend (the backend depends on it).
//!
//! - [`codec`]: SID-10 ByteCodecs, shared with the backend's pickle *writer*.
//! - [`classfile`]: just enough classfile parsing to reach `ScalaSignature`.
//! - [`read`]: pickle bytes to an entry table.
//! - [`sym`]: entry table to class signatures, with inheritance resolved.

pub mod classfile;
pub mod codec;
pub mod read;
pub mod sym;

pub use classfile::scala_signature_bytes;
pub use read::{read_pickle, Pickle, ReadError};
pub use sym::{class_sigs, ClassSig, ClassSource, Member, MemberHit, SigCache, SigLoader, SigType};
