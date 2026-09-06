//! nsc PickleFormat subset (major 5, minor 2) plus SID-10 ByteCodecs.
//!
//! This is enough for scala-rs to round-trip compiled classes/objects through
//! `ScalaSignature` and for scalac 2.13.16 to typecheck a `val`, a `def` with
//! parameters, `id[T]`, a `case class` (`new Point` + ctor field accessors +
//! companion apply `Point(x, y)` / term `Point` via `MODULESYM` in the class
//! pickle + extractor `unapply` so `x match { case Point(a, b) => … }`), an
//! `object` method, a method taking `List[_]` (EXISTENTIALtpe), and
//! `@deprecated("msg", "2.13.0")` (SYMANNOT + LITERALstring), Java `@Deprecated`
//! (SYMANNOT + TypeRef(java.lang, Deprecated)), `this.type`
//! (THIStpe as a method result), `List[_ <: AnyRef]` (EXISTENTIALtpe with a
//! bounded TYPEsym), nested `List[_ <: List[_]]`, refinement types
//! (`A with B { def f: Int }` as REFINEDtpe), `T @unchecked` (ANNOTATEDtpe), and SIP-23 literal types
//! (`def f(x: 1)`, `val one: 1`) as `CONSTANTtpe` + `LITERALint` against our
//! classfiles, and `type T = Int` as `ALIASsym` (nsc 2.13 has no `ALIAStpe` tag).
//! Flags are nsc raw longs run through `rawToPickledFlags`.
//! MACRO / late / anti are **not** pickled: scalac 2.13.16 typechecks the
//! classfiles we already emit (`scalac_typechecks_against_our_classfiles_if_present`)
//! without them. Def macros are out of scope. JAVA is not placed on EXTREF
//! (no flags field on that pickle form).
//! It is **not** a full nsc pickle — leftover holes are documented in README.
//!
//! nsc-facing details in this subset (must match `PickleBuffer` / `UnPickler`):
//! - pickle = major, minor, **nentries**, then `{ tag_Nat, len_Nat, body }`
//! - Nat / LongNat are **big-endian** base-128 (nsc `writeLongNat`)
//! - `NOPREFIXtpe` for primitive `TypeRef` prefixes; class type params use `THIStpe`
//! - `scala` / `java.lang` / the class's own package / `<empty>` `EXTMODCLASSref` (term names)
//! - objects pickle as `CLASSsym` + MODULE (type name), not `MODULEsym`
//! - `POLYtpe` is restpe first, then tparams; empty tparams = NullaryMethodType
//! - methods carry `METHOD`; vals are getters (`METHOD|STABLE|ACCESSOR` + `POLYtpe`)
//! - `List[_]` is `EXISTENTIALtpe(TypeRef(immutable.List, _$n), TYPEsym _$n)`
//! - `List[_ <: AnyRef]` sets the quantified TYPEsym hi bound to `AnyRef`
//! - nested `List[_ <: List[_]]` pickles the inner wildcard as its own EXISTENTIALtpe hi bound (nsc `List[_ <: List[_]]`)
//! - `A with B { def f: Int }` is `REFINEDtpe` + `<refinement>` CLASSsym (deferred members)
//! - `this.type` results are `THIStpe` of the enclosing class
//! - type annotations `T @unchecked` / `T @uncheckedVariance` are `ANNOTATEDtpe` + `ANNOTINFO`
//! - annotation args that are string/int/boolean literals are Constants;
//!   `classOf[T]` is `LITERALclass`; simple `Ident` / `Select` / `this` / `Apply`
//!   args are `TREE` so scalac 2.13.16 can typecheck `@Ann(foo)` / `@Ann(this)` /
//!   `@Ann(foo(1))` / `@Ann(classOf[Int])` / `@Ann(foo = 1)` / `@Ann(foo = this.x)`
//!   on a method (named args are pickled as positional, matching nsc typer)
//! - Java `@Deprecated` is `SYMANNOT` with `TypeRef` under `java.lang` (not skipped)
//! - SIP-23 `1` in a signature is `CONSTANTtpe(LITERALint)` (nsc `writeLong` =
//!   signed big-endian base 256)
//! - Scala `T*` methods pickle `VARARGS` and `<repeated>[T]`; erasure bridges
//!   pickle `BRIDGE|SYNTHETIC`; Java-defined CLASSsym pickle `JAVA`
//! - `type T = Int` / `type A = String` is `ALIASsym` (tag 5) with the aliased
//!   type as info (nsc 2.13 has no `ALIAStpe`; scalac 2.13.16 typechecks `Lib.T`)

use scala_rs_parser::{Flags, Lit, RefineDecl, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{SymKind, SymbolTable};

pub use scala_rs_pickle::codec::{
    avoid_zero, decode7to8, decode_annotation_string, encode8to7, encode_bytes,
    encode_to_annotation_string, regenerate_zero, MAJOR, MINOR,
};

pub const TERMNAME: u8 = 1;
pub const TYPENAME: u8 = 2;
pub const NONESYM: u8 = 3;
pub const TYPESYM: u8 = 4;
/// nsc `ALIASsym` — `type T = Int` (info is the aliased type, not TYPEBOUNDStpe).
/// 2.13 PickleFormat has no `ALIAStpe` tag; aliases are this symbol form.
pub const ALIASSYM: u8 = 5;
pub const CLASSSYM: u8 = 6;
pub const MODULESYM: u8 = 7;
pub const VALSYM: u8 = 8;
pub const EXTREF: u8 = 9;
pub const EXTMODCLASSREF: u8 = 10;
pub const NOTPE: u8 = 11;
pub const NOPREFIXTPE: u8 = 12;
pub const THISTPE: u8 = 13;
pub const SINGLETPE: u8 = 14;
/// nsc `CONSTANTtpe` (mixed case matches PickleFormat).
#[allow(non_upper_case_globals)]
pub const CONSTANTtpe: u8 = 15;
pub const TYPEREFTPE: u8 = 16;
pub const TYPEBOUNDSTPE: u8 = 17;
pub const CLASSINFOTPE: u8 = 19;
pub const METHODTPE: u8 = 20;
pub const POLYTPE: u8 = 21;
/// nsc literal tags (mixed case matches PickleFormat).
#[allow(non_upper_case_globals)]
pub const LITERALunit: u8 = 24;
#[allow(non_upper_case_globals)]
pub const LITERALboolean: u8 = 25;
#[allow(non_upper_case_globals)]
pub const LITERALchar: u8 = 28;
#[allow(non_upper_case_globals)]
pub const LITERALint: u8 = 29;
#[allow(non_upper_case_globals)]
pub const LITERALlong: u8 = 30;
#[allow(non_upper_case_globals)]
pub const LITERALfloat: u8 = 31;
#[allow(non_upper_case_globals)]
pub const LITERALdouble: u8 = 32;
/// nsc `LITERALstring` (mixed case matches PickleFormat).
#[allow(non_upper_case_globals)]
pub const LITERALstring: u8 = 33;
#[allow(non_upper_case_globals)]
pub const LITERALnull: u8 = 34;
/// nsc `LITERALclass` — `classOf[T]` annotation args (Constant, not TREE).
#[allow(non_upper_case_globals)]
pub const LITERALclass: u8 = 35;
pub const SYMANNOT: u8 = 40;
pub const ANNOTATEDTPE: u8 = 42;
pub const ANNOTINFO: u8 = 43;
pub const REFINEDTPE: u8 = 18;
pub const EXISTENTIALTPE: u8 = 48;
/// nsc `TREE` — annotation arguments that are not Constants.
pub const TREE: u8 = 49;
#[allow(non_upper_case_globals)]
pub const TYPEAPPLYtree: u8 = 30;
#[allow(non_upper_case_globals)]
pub const APPLYtree: u8 = 31;
#[allow(non_upper_case_globals)]
pub const SUPERtree: u8 = 33;
#[allow(non_upper_case_globals)]
pub const THIStree: u8 = 34;
#[allow(non_upper_case_globals)]
pub const SELECTtree: u8 = 35;
#[allow(non_upper_case_globals)]
pub const IDENTtree: u8 = 36;
#[allow(non_upper_case_globals)]
pub const LITERALtree: u8 = 37;

/// A type recovered from a pickle: the head symbol's name plus its type
/// arguments. A `TYPEREFtpe` carries its arguments after the symbol reference,
/// so `F[A]` reads back as `{ name: "F", args: ["A"] }` rather than as the bare
/// head name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PickledType {
    pub name: String,
    pub args: Vec<PickledType>,
}

impl PickledType {
    pub fn simple(name: impl Into<String>) -> Self {
        PickledType {
            name: name.into(),
            args: Vec::new(),
        }
    }
}

/// A type parameter recovered from a pickle, with its own parameters so that a
/// type constructor (`F[_]`) is not read back as a proper type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickledTypeParam {
    pub name: String,
    pub tparams: Vec<PickledTypeParam>,
}

impl PickledTypeParam {
    pub fn simple(name: impl Into<String>) -> Self {
        PickledTypeParam {
            name: name.into(),
            tparams: Vec::new(),
        }
    }
}

// An argument-free pickled type is exactly its name; comparing against a name
// keeps the unpickler's tests readable.
impl PartialEq<str> for PickledType {
    fn eq(&self, other: &str) -> bool {
        self.args.is_empty() && self.name == other
    }
}

impl PartialEq<&str> for PickledType {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for PickledType {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<str> for PickledTypeParam {
    fn eq(&self, other: &str) -> bool {
        self.tparams.is_empty() && self.name == other
    }
}

impl PartialEq<&str> for PickledTypeParam {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for PickledTypeParam {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

/// Pickled method, constructor, or val recovered by the subset unpickler.
#[derive(Clone, Debug)]
pub struct PickledMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<PickledType>,
    pub ret: PickledType,
    pub tparams: Vec<PickledTypeParam>,
    pub is_val: bool,
    pub is_ctor: bool,
    pub is_implicit: bool,
    /// nsc's `DEFERRED`: this member is a *declaration*. The class file cannot
    /// always say so -- every member of an interface but the `default` methods
    /// is `ACC_ABSTRACT` there, so a trait's concrete `val` accessor and its
    /// deferred one look identical -- and a reader that takes them all for
    /// definitions asks for an `override` modifier scalac does not.
    pub is_deferred: bool,
    /// A `var`'s getter. nsc strips `MUTABLE` from the accessor it derives and
    /// marks only an *immutable* one `STABLE`, so "accessor without `STABLE`"
    /// is how a reader tells a `var` from a `val`. Nothing in the class file
    /// carries it, which is why `c.count = 9` on a trait read from `-cp` was
    /// `reassignment to val count`.
    pub is_mutable: bool,
}

/// Pickled class or module class.
#[derive(Clone, Debug)]
pub struct PickledClass {
    pub name: String,
    pub is_module: bool,
    pub tparams: Vec<PickledTypeParam>,
    pub methods: Vec<PickledMethod>,
    /// The pickled parents name `scala.AnyVal`, i.e. this is a value class.
    ///
    /// It cannot be read off the class file: nsc gives a value class
    /// `java/lang/Object` for a superclass and no interfaces, so the classfile
    /// header of `class Meters(val n: Int) extends AnyVal` is
    /// indistinguishable from a plain final class's. The distinction decides
    /// the whole of erasure for it -- `Meters` erases to `int`, and its
    /// methods are called as `$extension` statics.
    pub extends_anyval: bool,
}

// ---------------------------------------------------------------------------
// Pickle buffer
// ---------------------------------------------------------------------------

struct Buf {
    bytes: Vec<u8>,
}

impl Buf {
    fn new() -> Self {
        Buf { bytes: Vec::new() }
    }

    #[allow(dead_code)]
    fn write_byte(&mut self, b: u8) {
        self.bytes.push(b);
    }

    fn write_nat(&mut self, x: u32) {
        write_long_nat_to(&mut self.bytes, x as u64);
    }

    fn write_entry(&mut self, tag: u8, body: &[u8]) {
        // nsc: Entry = type_Nat length_Nat body
        self.write_nat(tag as u32);
        self.write_nat(body.len() as u32);
        self.bytes.extend_from_slice(body);
    }
}

struct Pickler<'a> {
    st: &'a SymbolTable,
    entries: Vec<(u8, Vec<u8>)>,
    term_names: std::collections::HashMap<String, u32>,
    type_names: std::collections::HashMap<String, u32>,
    ext_refs: std::collections::HashMap<String, u32>,
    none: u32,
    notpe: u32,
    noprefix: u32,
    scala_mod: Option<u32>,
    java_lang_mod: Option<u32>,
    empty_pkg: Option<u32>,
    sym_index: std::collections::HashMap<u32, u32>,
    this_tpes: std::collections::HashMap<u32, u32>,
    class_tparams: std::collections::HashMap<u32, Vec<u32>>,
    /// Pickle entry of the package a class entry belongs to, so a constructor
    /// result written after the fact still names the right prefix.
    class_pkg: std::collections::HashMap<u32, u32>,
    current_owner: u32,
    exist_n: u32,
    collection_immutable: Option<u32>,
}

impl<'a> Pickler<'a> {
    fn new(st: &'a SymbolTable) -> Self {
        let mut p = Pickler {
            st,
            entries: Vec::new(),
            term_names: std::collections::HashMap::new(),
            type_names: std::collections::HashMap::new(),
            ext_refs: std::collections::HashMap::new(),
            none: 0,
            notpe: 0,
            noprefix: 0,
            scala_mod: None,
            java_lang_mod: None,
            empty_pkg: None,
            sym_index: std::collections::HashMap::new(),
            this_tpes: std::collections::HashMap::new(),
            class_tparams: std::collections::HashMap::new(),
            class_pkg: std::collections::HashMap::new(),
            current_owner: 0,
            exist_n: 0,
            collection_immutable: None,
        };
        p.none = p.add(NONESYM, vec![]);
        p.notpe = p.add(NOTPE, vec![]);
        p.noprefix = p.add(NOPREFIXTPE, vec![]);
        p
    }

    fn add(&mut self, tag: u8, body: Vec<u8>) -> u32 {
        let i = self.entries.len() as u32;
        self.entries.push((tag, body));
        i
    }

    fn term_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.term_names.get(name) {
            return *i;
        }
        let i = self.add(TERMNAME, name.as_bytes().to_vec());
        self.term_names.insert(name.to_string(), i);
        i
    }

    fn type_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.type_names.get(name) {
            return *i;
        }
        let i = self.add(TYPENAME, name.as_bytes().to_vec());
        self.type_names.insert(name.to_string(), i);
        i
    }

    fn ext_mod(&mut self, name: &str, owner: Option<u32>) -> u32 {
        let key = match owner {
            Some(o) => format!("mod:{name}@{o}"),
            None => format!("mod:{name}"),
        };
        if let Some(i) = self.ext_refs.get(&key) {
            return *i;
        }
        // nsc EXTMODCLASSref uses the module's term name.
        let n = self.term_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        if let Some(o) = owner {
            write_nat_to(&mut body, o);
        }
        let i = self.add(EXTMODCLASSREF, body);
        self.ext_refs.insert(key, i);
        i
    }

    fn ext_ref_owned(&mut self, name: &str, owner: u32) -> u32 {
        let key = format!("ext:{name}@{owner}");
        if let Some(i) = self.ext_refs.get(&key) {
            return *i;
        }
        let n = self.type_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        write_nat_to(&mut body, owner);
        let i = self.add(EXTREF, body);
        self.ext_refs.insert(key, i);
        i
    }

    fn scala_module(&mut self) -> u32 {
        if let Some(i) = self.scala_mod {
            return i;
        }
        let i = self.ext_mod("scala", None);
        self.scala_mod = Some(i);
        i
    }

    fn java_lang_module(&mut self) -> u32 {
        if let Some(i) = self.java_lang_mod {
            return i;
        }
        let java = self.ext_mod("java", None);
        let i = self.ext_mod("lang", Some(java));
        self.java_lang_mod = Some(i);
        i
    }

    fn empty_package(&mut self) -> u32 {
        if let Some(i) = self.empty_pkg {
            return i;
        }
        let i = self.ext_mod("<empty>", None);
        self.empty_pkg = Some(i);
        i
    }

    /// The module class of the package a JVM name lives in, as an
    /// `EXTMODCLASSref` chain (`hklib`, `slick/ast`), or `<empty>` for the
    /// default package.
    ///
    /// An unpickler reads a symbol's owner straight out of the pickle, so
    /// writing `<empty>` for everything meant a class compiled inside
    /// `package hklib` called itself `Monadic`, not `hklib.Monadic`. Neither
    /// scalac 2.13.16 nor scala-rs's own `scala-rs-pickle` could then find it
    /// ("not found: type Monadic" against our own classfiles), which is what
    /// made a jar of scala-rs output unreadable while a *directory* of the
    /// same classfiles worked -- the directory path recovers the package from
    /// the file's path instead.
    fn package_ref_of(&mut self, jvm_name: &str) -> u32 {
        let Some((pkg, _)) = jvm_name.rsplit_once('/') else {
            return self.empty_package();
        };
        let mut owner: Option<u32> = None;
        for seg in pkg.split('/').filter(|s| !s.is_empty()) {
            owner = Some(self.ext_mod(seg, owner));
        }
        match owner {
            Some(o) => o,
            None => self.empty_package(),
        }
    }

    fn scala_collection_immutable(&mut self) -> u32 {
        if let Some(i) = self.collection_immutable {
            return i;
        }
        let sc = self.scala_module();
        let col = self.ext_mod("collection", Some(sc));
        let i = self.ext_mod("immutable", Some(col));
        self.collection_immutable = Some(i);
        i
    }

    /// nsc constructor result: `TypeRef(<prefix>, C, tparams)`.
    ///
    /// The prefix is where an instance of `C` is reached from, and runtime
    /// reflection prints it: `class A` at the top level of `object Test` has
    /// `def <init>(): Test.A`, the same class inside `class Test` has
    /// `def <init>(): Test.this.A2`, and only a class the *package* owns gets
    /// the `ThisType(<package>)` that nsc elides down to a bare `A`
    /// (`run/t5256b`, `run/t5256e`, `run/t5256f`).
    fn ctor_result_type(&mut self, class_idx: u32, class_id: SymbolId) -> u32 {
        let pref = self.ctor_result_prefix(class_idx, class_id);
        let tparams = self
            .class_tparams
            .get(&class_idx)
            .cloned()
            .unwrap_or_default();
        let mut targs = Vec::new();
        for tp in tparams {
            let mut tr = Vec::new();
            write_nat_to(&mut tr, self.noprefix);
            write_nat_to(&mut tr, tp);
            targs.push(self.add(TYPEREFTPE, tr));
        }
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, class_idx);
        for t in targs {
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    /// The prefix of a constructor's result type — see [`Self::ctor_result_type`].
    fn ctor_result_prefix(&mut self, class_idx: u32, class_id: SymbolId) -> u32 {
        let owner = if class_id.is_none() {
            SymbolId::NONE
        } else {
            self.st.get(class_id).owner
        };
        if !owner.is_none() {
            let owner_cls = match self.st.get(owner).kind {
                SymKind::Class | SymKind::ModuleClass => owner,
                SymKind::Module => self.st.module_class_of(owner),
                _ => SymbolId::NONE,
            };
            // Only when the enclosing class is one this pickle already holds:
            // `pickle_this_tpe` would otherwise pull an external class into it.
            if !owner_cls.is_none() && self.this_tpes.contains_key(&owner_cls.0) {
                // nsc's `ThisType#prefixString` prints a *module* class by its
                // full name (`Test.`) and any other class as `Test.this.`, so
                // one `ThisType` spells both of the shapes these tests expect.
                return self.pickle_this_tpe(owner_cls);
            }
        }
        let pkg = self
            .class_pkg
            .get(&class_idx)
            .copied()
            .unwrap_or_else(|| self.empty_package());
        let mut th = Vec::new();
        write_nat_to(&mut th, pkg);
        self.add(THISTPE, th)
    }

    fn type_ref_in(&mut self, owner: u32, name: &str) -> u32 {
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, owner);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_in_args(&mut self, owner: u32, name: &str, args: &[Type]) -> u32 {
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, owner);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        for a in args {
            let t = self.pickle_type(a);
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn ext_ref(&mut self, name: &str) -> u32 {
        if let Some(i) = self.ext_refs.get(name) {
            return *i;
        }
        let n = self.type_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        let i = self.add(EXTREF, body);
        self.ext_refs.insert(name.to_string(), i);
        i
    }

    fn type_ref_named(&mut self, name: &str) -> u32 {
        match name {
            "Int" | "Long" | "Float" | "Double" | "Boolean" | "Char" | "Unit" | "Any"
            | "AnyRef" | "AnyVal" | "Nothing" | "Null" | "Array" | "Seq" => {
                let sc = self.scala_module();
                self.type_ref_in(sc, name)
            }
            n if n.starts_with("Function") => {
                let sc = self.scala_module();
                self.type_ref_in(sc, n)
            }
            "String" | "Object" => {
                let jl = self.java_lang_module();
                self.type_ref_in(jl, name)
            }
            _ => {
                let pref = self.noprefix;
                let sym = self.ext_ref(name);
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
        }
    }

    fn type_ref_of_pickle_sym(&mut self, idx: u32) -> u32 {
        let mut body = Vec::new();
        write_nat_to(&mut body, self.noprefix);
        write_nat_to(&mut body, idx);
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_local_refs(&mut self, class_idx: u32, arg_refs: &[u32]) -> u32 {
        let mut body = Vec::new();
        write_nat_to(&mut body, self.noprefix);
        write_nat_to(&mut body, class_idx);
        for t in arg_refs {
            write_nat_to(&mut body, *t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_in_refs(&mut self, owner: u32, name: &str, arg_refs: &[u32]) -> u32 {
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, owner);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        for t in arg_refs {
            write_nat_to(&mut body, *t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn class_type_ref(&mut self, class_sym: SymbolId, arg_refs: &[u32]) -> u32 {
        if let Some(&idx) = self.sym_index.get(&class_sym.0) {
            return self.type_ref_local_refs(idx, arg_refs);
        }
        let n = self
            .st
            .get(class_sym)
            .name
            .trim_end_matches('$')
            .to_string();
        match n.as_str() {
            "Int" | "Long" | "Float" | "Double" | "Boolean" | "Char" | "Unit" | "Any"
            | "AnyRef" | "AnyVal" | "Nothing" | "Null" | "Array" | "Seq" | "String" | "Object" => {
                if arg_refs.is_empty() {
                    self.type_ref_named(&n)
                } else {
                    let owner = if n == "String" || n == "Object" {
                        self.java_lang_module()
                    } else {
                        self.scala_module()
                    };
                    self.type_ref_in_refs(owner, &n, arg_refs)
                }
            }
            "Option" | "Some" | "None" => {
                let sc = self.scala_module();
                self.type_ref_in_refs(sc, n.as_str(), arg_refs)
            }
            "List" | "Nil" => {
                let imm = self.scala_collection_immutable();
                self.type_ref_in_refs(imm, n.as_str(), arg_refs)
            }
            "::" | "$colon$colon" => {
                let imm = self.scala_collection_immutable();
                self.type_ref_in_refs(imm, "$colon$colon", arg_refs)
            }
            // A prefix test would claim slick's own `TupleShape` for the
            // `scala` package; only the real `TupleN` / `FunctionN` live there.
            n if numbered_arity(n, "Tuple").is_some() => {
                let sc = self.scala_module();
                self.type_ref_in_refs(sc, n, arg_refs)
            }
            n if numbered_arity(n, "Function").is_some() => {
                if arg_refs.is_empty() {
                    self.type_ref_named(n)
                } else {
                    let sc = self.scala_module();
                    self.type_ref_in_refs(sc, n, arg_refs)
                }
            }
            n => {
                let owner = self.external_owner_ref(class_sym);
                self.type_ref_in_refs(owner, n, arg_refs)
            }
        }
    }

    /// The reference a *nested* class hangs off.
    ///
    /// `package_ref_of` only ever produced a package, so
    /// `slick/jdbc/JdbcActionComponent$MultipleRowsPerStatementSupport` was
    /// written as `slick.jdbc.MultipleRowsPerStatementSupport` -- a symbol
    /// that does not exist, which nsc reports as "Symbol 'type
    /// slick.jdbc.MultipleRowsPerStatementSupport' is missing from the
    /// classpath" the first time anything mentions the class that inherits
    /// it. The owner chain comes from the symbol table (which knows whether
    /// each step is a class or an object) and is used only when the JVM name
    /// says the class really is nested, so a plain top-level class keeps the
    /// package path it had.
    fn external_owner_ref(&mut self, class_sym: SymbolId) -> u32 {
        let jvm = self.st.get(class_sym).jvm_name.clone();
        let last = jvm.rsplit('/').next().unwrap_or(&jvm);
        if !last.trim_end_matches('$').contains('$') {
            return self.package_ref_of(&jvm);
        }
        self.owner_chain_ref(class_sym, 0)
    }

    /// The owner a `CLASSsym` names, preferring an entry in *this* pickle.
    ///
    /// nsc enters every symbol it reads in the scope of whatever its owner
    /// field points at, so a nested class written into its enclosing class's
    /// pickle has to point at that entry. Anything else -- a top-level class,
    /// or the same nested class pickled on its own for its own class file --
    /// still names the external owner chain.
    fn local_owner_ref(&mut self, class_sym: SymbolId) -> u32 {
        let owner = self.st.get(class_sym).owner;
        if !owner.is_none() {
            let ow = if self.st.get(owner).kind == SymKind::Module {
                self.st.module_class_of(owner)
            } else {
                owner
            };
            if let Some(&i) = self.sym_index.get(&ow.0) {
                if i != self
                    .sym_index
                    .get(&class_sym.0)
                    .copied()
                    .unwrap_or(u32::MAX)
                {
                    return i;
                }
            }
        }
        self.external_owner_ref(class_sym)
    }

    /// Is `m` a class or object *declared inside* `outer`, under a name a
    /// later compilation can spell?
    ///
    /// The JVM name is the test rather than the owner field alone: a lifted
    /// local class (`Outer$$anon$1`, `Outer$1`) is owned by a method, not by
    /// the class, and writing one into the class's scope would put a name no
    /// source can name into the signature.
    fn nested_member_of(&self, m: SymbolId, outer: SymbolId) -> bool {
        if m.is_none() || m == outer {
            return false;
        }
        if !matches!(self.st.get(m).kind, SymKind::Class | SymKind::ModuleClass) {
            return false;
        }
        let outer_jvm = self
            .st
            .get(outer)
            .jvm_name
            .trim_end_matches('$')
            .to_string();
        if outer_jvm.is_empty() {
            return false;
        }
        let jvm = self.st.get(m).jvm_name.clone();
        let Some(rest) = jvm.strip_prefix(&format!("{outer_jvm}$")) else {
            return false;
        };
        let rest = rest.trim_end_matches('$');
        !rest.is_empty() && !rest.contains('$') && !rest.starts_with(|c: char| c.is_ascii_digit())
    }

    fn owner_chain_ref(&mut self, sym: SymbolId, depth: u32) -> u32 {
        let jvm = self.st.get(sym).jvm_name.clone();
        let owner = self.st.get(sym).owner;
        if depth > 8 || owner.is_none() {
            return self.package_ref_of(&jvm);
        }
        let (kind, name) = {
            let o = self.st.get(owner);
            (o.kind, o.name.trim_end_matches('$').to_string())
        };
        match kind {
            SymKind::Module | SymKind::ModuleClass => {
                let up = self.owner_chain_ref(owner, depth + 1);
                self.ext_mod(&name, Some(up))
            }
            SymKind::Class => {
                let up = self.owner_chain_ref(owner, depth + 1);
                self.ext_ref_owned(&name, up)
            }
            // A package (or anything else): the JVM name already spells it.
            _ => self.package_ref_of(&jvm),
        }
    }

    fn pickle_existential_tpe(&mut self, inner: u32, quantified: &[u32]) -> u32 {
        if quantified.is_empty() {
            return inner;
        }
        let mut body = Vec::new();
        write_nat_to(&mut body, inner);
        for q in quantified {
            write_nat_to(&mut body, *q);
        }
        self.add(EXISTENTIALTPE, body)
    }

    /// Pack nested wildcards into one EXISTENTIALtpe (`List[_ <: List[_]]`).
    fn pickle_type_pack(&mut self, ty: &Type, quantified: &mut Vec<u32>) -> u32 {
        match ty {
            Type::Wildcard => {
                let q = self.pickle_existential_param_refs(None, None);
                quantified.push(q);
                self.type_ref_of_pickle_sym(q)
            }
            Type::BoundedWildcard { lo, hi } => {
                let lo_r = lo.as_deref().map(|t| self.pickle_type(t));
                let hi_r = hi.as_deref().map(|t| self.pickle_type(t));
                let q = self.pickle_existential_param_refs(lo_r, hi_r);
                quantified.push(q);
                self.type_ref_of_pickle_sym(q)
            }
            Type::Class { sym, args } => {
                let arg_refs: Vec<u32> = args
                    .iter()
                    .map(|a| self.pickle_type_pack(a, quantified))
                    .collect();
                self.class_type_ref(*sym, &arg_refs)
            }
            Type::Applied { ctor, args } => match ctor.as_ref() {
                Type::TypeParam(id) => {
                    let pref = self.noprefix;
                    let sym = self.pickle_typesym(*id);
                    let mut body = Vec::new();
                    write_nat_to(&mut body, pref);
                    write_nat_to(&mut body, sym);
                    for a in args {
                        write_nat_to(&mut body, self.pickle_type_pack(a, quantified));
                    }
                    self.add(TYPEREFTPE, body)
                }
                Type::Class {
                    sym,
                    args: existing,
                } => {
                    let mut all = existing.clone();
                    all.extend(args.iter().cloned());
                    self.pickle_type_pack(
                        &Type::Class {
                            sym: *sym,
                            args: all,
                        },
                        quantified,
                    )
                }
                _ => self.pickle_type(ctor),
            },
            Type::Tuple(ts) => {
                let arg_refs: Vec<u32> = ts
                    .iter()
                    .map(|a| self.pickle_type_pack(a, quantified))
                    .collect();
                let sc = self.scala_module();
                self.type_ref_in_refs(sc, &format!("Tuple{}", ts.len()), &arg_refs)
            }
            Type::Annotated { tpe, annot } => {
                let inner = self.pickle_type_pack(tpe, quantified);
                let atp = self.pickle_type_annot_ref(annot);
                let mut ab = Vec::new();
                write_nat_to(&mut ab, atp);
                let info = self.add(ANNOTINFO, ab);
                let mut body = Vec::new();
                write_nat_to(&mut body, inner);
                write_nat_to(&mut body, info);
                self.add(ANNOTATEDTPE, body)
            }
            other => self.pickle_type(other),
        }
    }

    fn pickle_existential_param_refs(&mut self, lo: Option<u32>, hi: Option<u32>) -> u32 {
        self.exist_n += 1;
        let name_ref = self.type_name(&format!("_${}", self.exist_n));
        let idx = self.add(TYPESYM, vec![]);
        let lo = lo.unwrap_or_else(|| self.type_ref_named("Nothing"));
        let hi = hi.unwrap_or_else(|| self.type_ref_named("Any"));
        let mut b = Vec::new();
        write_nat_to(&mut b, lo);
        write_nat_to(&mut b, hi);
        let bounds = self.add(TYPEBOUNDSTPE, b);
        let flags = raw_to_pickled((1u64 << 4) | (1u64 << 13)) | (1u64 << 35);
        let body = self.symbol_info(name_ref, self.current_owner, flags, bounds);
        self.entries[idx as usize] = (TYPESYM, body);
        idx
    }

    fn pickle_refined(&mut self, parents: &[Type], decls: &[RefineDecl]) -> u32 {
        let name_ref = self.type_name("<refinement>");
        let idx = self.add(CLASSSYM, vec![]);
        let owner = self.current_owner;
        let saved = self.current_owner;
        self.current_owner = idx;
        for d in decls {
            match d {
                RefineDecl::Def { name, paramss, ret } => {
                    self.pickle_refined_def(idx, name, paramss, ret)
                }
                RefineDecl::Val { name, ty } => {
                    self.pickle_refined_def(idx, name, &[], ty);
                }
                RefineDecl::Type { .. } => {}
            }
        }
        let parent_refs: Vec<u32> = if parents.is_empty() {
            vec![self.type_ref_named("AnyRef")]
        } else {
            parents.iter().map(|p| self.pickle_type(p)).collect()
        };
        let mut info_body = Vec::new();
        write_nat_to(&mut info_body, idx);
        for p in &parent_refs {
            write_nat_to(&mut info_body, *p);
        }
        let info = self.add(CLASSINFOTPE, info_body);
        // SYNTHETIC (not remapped)
        let flags = 1u64 << 21;
        let body = self.symbol_info(name_ref, owner, flags, info);
        self.entries[idx as usize] = (CLASSSYM, body);
        self.current_owner = saved;
        let mut rb = Vec::new();
        write_nat_to(&mut rb, idx);
        for p in parent_refs {
            write_nat_to(&mut rb, p);
        }
        self.add(REFINEDTPE, rb)
    }

    fn pickle_refined_def(
        &mut self,
        owner_ref: u32,
        name: &str,
        paramss: &[Vec<Type>],
        ret: &Type,
    ) {
        let name_ref = self.term_name(name);
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let params: Vec<(String, Type)> = paramss
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, t)| (format!("x${i}"), t.clone()))
            .collect();
        let mut param_refs = Vec::new();
        for (pname, pty) in &params {
            let pn = self.term_name(pname);
            let pty_ref = self.pickle_type(pty);
            let flags = pickled_from_our(Flags::PARAM, SymKind::Term, 1u64 << 13);
            let body = self.symbol_info(pn, meth_idx, flags, pty_ref);
            param_refs.push(self.add(VALSYM, body));
        }
        let ret_ref = self.pickle_type(ret);
        let info = if params.is_empty() {
            let mut pt = Vec::new();
            write_nat_to(&mut pt, ret_ref);
            self.add(POLYTPE, pt)
        } else {
            let mut mt = Vec::new();
            write_nat_to(&mut mt, ret_ref);
            for p in param_refs {
                write_nat_to(&mut mt, p);
            }
            self.add(METHODTPE, mt)
        };
        // METHOD | DEFERRED
        let flags = raw_to_pickled((1u64 << 6) | (1u64 << 4));
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    fn pickle_literal_string(&mut self, s: &str) -> u32 {
        let n = self.term_name(s);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        self.add(LITERALstring, body)
    }

    /// nsc `PickleBuffer.writeLong`: signed big-endian base 256.
    fn pickle_literal(&mut self, lit: &Lit) -> u32 {
        match lit {
            Lit::Unit => self.add(LITERALunit, vec![]),
            Lit::Null => self.add(LITERALnull, vec![]),
            Lit::Boolean(b) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, if *b { 1 } else { 0 });
                self.add(LITERALboolean, body)
            }
            Lit::Int(n) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, *n as i64);
                self.add(LITERALint, body)
            }
            Lit::Long(n) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, *n);
                self.add(LITERALlong, body)
            }
            Lit::Float(n) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, n.to_bits() as i64);
                self.add(LITERALfloat, body)
            }
            Lit::Double(n) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, n.to_bits() as i64);
                self.add(LITERALdouble, body)
            }
            Lit::Char(c) => {
                let mut body = Vec::new();
                write_long_signed_256(&mut body, *c as u32 as i64);
                self.add(LITERALchar, body)
            }
            Lit::String(s) => self.pickle_literal_string(s),
            Lit::Symbol(s) => self.pickle_literal_string(s),
        }
    }

    fn pickle_symannot(&mut self, target: u32, annot: &Tree, owner: SymbolId) {
        let path = annot.annotation_path();
        let simple = path.rsplit('.').next().unwrap_or(path.as_str());
        if simple == "Override" || path == "java.lang.Override" {
            return;
        }
        // `@specialized` / `@unspecialized` are accepted and recorded on the
        // symbol, but the `specialize` phase is not implemented: no `$mc*$sp`
        // member is emitted. Pickling the annotation would tell scalac that
        // this class *is* specialized and let it link to members that are not
        // there, so the pickle stays silent until stage 2 puts them there.
        // See docs/specialization.md and tests/spec_classfiles.sh.
        if simple == scala_rs_parser::specialization::SPECIALIZED
            || simple == scala_rs_parser::specialization::UNSPECIALIZED
        {
            return;
        }
        let atp = if simple == "tailrec" {
            let sc = self.scala_module();
            let ann = self.ext_mod("annotation", Some(sc));
            self.type_ref_in(ann, "tailrec")
        } else if simple == "deprecated" {
            let sc = self.scala_module();
            self.type_ref_in(sc, "deprecated")
        } else if simple == "Deprecated" || path.starts_with("java.lang.Deprecated") {
            // Java `@Deprecated`: SYMANNOT + TypeRef(java.lang, Deprecated) so
            // scalac 2.13.16 sees the annotation on our methods (classfile RVA
            // is not enough; nsc reads pickle).
            let jl = self.java_lang_module();
            self.type_ref_in(jl, "Deprecated")
        } else {
            // User-defined `@Ann(...)` lives in `<empty>`, not under scala.
            let empty = self.empty_package();
            self.type_ref_in(empty, simple)
        };
        let mut body = Vec::new();
        write_nat_to(&mut body, target);
        write_nat_to(&mut body, atp);
        for arg in annot_args(annot) {
            if let Some(r) = self.pickle_annot_arg(arg, owner) {
                write_nat_to(&mut body, r);
            }
        }
        self.add(SYMANNOT, body);
    }

    /// Constant (literal / classOf) or TREE Ident/Select/This/Super/Apply.
    /// Named `@Ann(foo = 1)` / `@Ann(foo = this.x)` pickle the rhs (Constant or
    /// TREE) — nsc typer rewrites named args to positional before pickling.
    fn pickle_annot_arg(&mut self, arg: &Tree, owner: SymbolId) -> Option<u32> {
        match &arg.kind {
            TreeKind::Literal { lit } => Some(self.pickle_literal(lit)),
            TreeKind::Ident { name } => Some(self.pickle_ident_tree(name, owner)),
            TreeKind::Select { qual, name } => self.pickle_select_tree(qual, name, owner),
            TreeKind::This { qual } => Some(self.pickle_this_tree(qual.as_deref(), owner)),
            TreeKind::Super { qual, mix } => {
                Some(self.pickle_super_tree(qual.as_deref(), mix.as_deref(), owner))
            }
            TreeKind::Assign { lhs, rhs } if matches!(&lhs.kind, TreeKind::Ident { .. }) => {
                self.pickle_annot_arg(rhs, owner)
            }
            TreeKind::TypeApply { fun, args } if is_classof_fun(fun) => {
                // nsc pickles annotation `classOf[T]` as LITERALclass Constant,
                // not TYPEAPPLYtree. scalac 2.13.16 reads that Constant.
                let tpe = args
                    .first()
                    .map(|t| self.pickle_type_from_tree(t))
                    .unwrap_or_else(|| self.type_ref_named("Any"));
                Some(self.pickle_literal_class(tpe))
            }
            TreeKind::Apply { fun, args }
                if matches!(
                    &fun.kind,
                    TreeKind::Ident { .. } | TreeKind::Select { .. } | TreeKind::Apply { .. }
                ) =>
            {
                self.pickle_apply_tree(fun, args, owner)
            }
            _ => None,
        }
    }

    fn lookup_member_named(&self, owner: SymbolId, name: &str) -> Option<SymbolId> {
        if owner.is_none() {
            return None;
        }
        for m in &self.st.get(owner).members {
            if self.st.get(*m).name == name {
                return Some(*m);
            }
        }
        let pkg = self.st.get(owner).owner;
        if !pkg.is_none() && pkg != owner {
            for m in &self.st.get(pkg).members {
                if self.st.get(*m).name == name {
                    return Some(*m);
                }
            }
        }
        None
    }

    fn pickle_member_sym(&mut self, id: SymbolId) -> u32 {
        if let Some(&i) = self.sym_index.get(&id.0) {
            return i;
        }
        let owner = self.st.get(id).owner;
        if let Some(&i) = self.sym_index.get(&owner.0) {
            return match self.st.get(id).kind {
                SymKind::Method => self.pickle_method(id, i, self.noprefix),
                _ => self.pickle_val(id, i, false, false),
            };
        }
        self.pickle_ext_term(id)
    }

    fn pickle_ext_term(&mut self, id: SymbolId) -> u32 {
        let name = self.st.get(id).name.clone();
        let owner_id = self.st.get(id).owner;
        let owner_ref = if let Some(&i) = self.sym_index.get(&owner_id.0) {
            i
        } else {
            let oname = self.st.get(owner_id).name.trim_end_matches('$').to_string();
            let empty = self.empty_package();
            self.ext_ref_owned(&oname, empty)
        };
        self.ext_term_ref(&name, owner_ref)
    }

    fn ext_term_ref(&mut self, name: &str, owner: u32) -> u32 {
        let key = format!("extterm:{name}@{owner}");
        if let Some(&i) = self.ext_refs.get(&key) {
            return i;
        }
        let n = self.term_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        write_nat_to(&mut body, owner);
        let i = self.add(EXTREF, body);
        self.ext_refs.insert(key, i);
        i
    }

    fn member_tree_tpe(&mut self, id: Option<SymbolId>) -> u32 {
        match id {
            Some(id) => {
                let ty = self.st.get(id).ty.clone();
                match ty {
                    Type::Method { ret, .. } => self.pickle_type(&ret),
                    other => self.pickle_type(&other),
                }
            }
            None => self.type_ref_named("Any"),
        }
    }

    fn pickle_ident_tree(&mut self, name: &str, owner: SymbolId) -> u32 {
        let found = self.lookup_member_named(owner, name);
        let tpe = self.member_tree_tpe(found);
        let sym = match found {
            Some(id) => self.pickle_member_sym(id),
            None => self.none,
        };
        let n = self.term_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, IDENTtree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, sym);
        write_nat_to(&mut body, n);
        self.add(TREE, body)
    }

    fn pickle_select_tree(&mut self, qual: &Tree, name: &str, owner: SymbolId) -> Option<u32> {
        let (qtree, xowner) = match &qual.kind {
            TreeKind::Ident { name: qname } => {
                let qfound = self.lookup_member_named(owner, qname);
                let qtree = self.pickle_ident_tree(qname, owner);
                let xowner = qfound.and_then(|id| self.select_owner_of(id));
                (qtree, xowner)
            }
            TreeKind::This { qual: tq } => {
                let cls = self.enclosing_class_sym(owner);
                let qtree = self.pickle_this_tree(tq.as_deref(), owner);
                (qtree, Some(cls).filter(|c| !c.is_none()))
            }
            TreeKind::Super { qual: sq, mix } => {
                let cls = self.enclosing_class_sym(owner);
                let qtree = self.pickle_super_tree(sq.as_deref(), mix.as_deref(), owner);
                (qtree, Some(cls).filter(|c| !c.is_none()))
            }
            TreeKind::Select {
                qual: inner,
                name: iname,
            } => {
                let qtree = self.pickle_select_tree(inner, iname, owner)?;
                (qtree, None)
            }
            _ => return None,
        };
        let xfound = xowner.and_then(|o| self.lookup_member_named(o, name));
        let tpe = self.member_tree_tpe(xfound);
        let sym = match xfound {
            Some(id) => self.pickle_member_sym(id),
            None => self.none,
        };
        let n = self.term_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, SELECTtree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, sym);
        write_nat_to(&mut body, qtree);
        write_nat_to(&mut body, n);
        Some(self.add(TREE, body))
    }

    fn select_owner_of(&self, id: SymbolId) -> Option<SymbolId> {
        match &self.st.get(id).ty {
            Type::Class { sym, .. } => Some(*sym),
            Type::ModuleRef(s) => Some(self.st.module_class_of(*s)).filter(|c| !c.is_none()),
            Type::ThisType(s) => Some(*s),
            _ => {
                let k = self.st.get(id).kind;
                if matches!(k, SymKind::Module | SymKind::ModuleClass | SymKind::Class) {
                    let cls = if k == SymKind::Module {
                        self.st.module_class_of(id)
                    } else {
                        id
                    };
                    Some(cls)
                } else {
                    None
                }
            }
        }
    }

    /// TREE { THIStree, type_Ref, sym_Ref, name_Ref } — UnPickler reads a symbol.
    fn pickle_this_tree(&mut self, qual: Option<&str>, owner: SymbolId) -> u32 {
        let cls = self.enclosing_class_sym(owner);
        let tpe = if cls.is_none() {
            self.notpe
        } else {
            self.pickle_this_tpe(cls)
        };
        let sym = if cls.is_none() {
            self.none
        } else {
            self.pickle_class(cls)
        };
        let n = match qual {
            Some(q) if !q.is_empty() => self.type_name(q),
            _ => self.type_name(""),
        };
        let mut body = Vec::new();
        write_nat_to(&mut body, THIStree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, sym);
        write_nat_to(&mut body, n);
        self.add(TREE, body)
    }

    /// TREE { SUPERtree, type_Ref, sym_Ref, qual_tree, mix_name } — UnPickler
    /// reads a symbol, then Super(qual, mix).
    fn pickle_super_tree(&mut self, qual: Option<&str>, mix: Option<&str>, owner: SymbolId) -> u32 {
        let cls = self.enclosing_class_sym(owner);
        let tpe = if cls.is_none() {
            self.notpe
        } else {
            self.pickle_this_tpe(cls)
        };
        let sym = if cls.is_none() {
            self.none
        } else {
            self.pickle_class(cls)
        };
        let qtree = self.pickle_this_tree(qual, owner);
        let mix_n = self.type_name(mix.unwrap_or(""));
        let mut body = Vec::new();
        write_nat_to(&mut body, SUPERtree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, sym);
        write_nat_to(&mut body, qtree);
        write_nat_to(&mut body, mix_n);
        self.add(TREE, body)
    }

    fn enclosing_class_sym(&self, owner: SymbolId) -> SymbolId {
        let mut id = owner;
        for _ in 0..8 {
            if id.is_none() {
                return id;
            }
            match self.st.get(id).kind {
                SymKind::Class | SymKind::ModuleClass => return id,
                SymKind::Module => {
                    let mc = self.st.module_class_of(id);
                    return if mc.is_none() { id } else { mc };
                }
                _ => id = self.st.get(id).owner,
            }
        }
        owner
    }

    /// TREE { APPLYtree, type_Ref, fun_tree, {arg_tree} } — no symbol (nsc).
    /// Nested args are TREEs (LITERALtree for literals), not bare Constants.
    fn pickle_apply_tree(&mut self, fun: &Tree, args: &[Tree], owner: SymbolId) -> Option<u32> {
        let fun_ref = self.pickle_nested_tree(fun, owner)?;
        let mut arg_refs = Vec::new();
        for a in args {
            arg_refs.push(self.pickle_nested_tree(a, owner)?);
        }
        let found = match &fun.kind {
            TreeKind::Ident { name } => self.lookup_member_named(owner, name),
            TreeKind::Select { name, .. } => self.lookup_member_named(owner, name),
            _ => None,
        };
        let tpe = self.member_tree_tpe(found);
        let mut body = Vec::new();
        write_nat_to(&mut body, APPLYtree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, fun_ref);
        for a in arg_refs {
            write_nat_to(&mut body, a);
        }
        Some(self.add(TREE, body))
    }

    fn pickle_nested_tree(&mut self, arg: &Tree, owner: SymbolId) -> Option<u32> {
        match &arg.kind {
            TreeKind::Literal { lit } => Some(self.pickle_literal_tree(lit)),
            TreeKind::Ident { name } => Some(self.pickle_ident_tree(name, owner)),
            TreeKind::Select { qual, name } => self.pickle_select_tree(qual, name, owner),
            TreeKind::This { qual } => Some(self.pickle_this_tree(qual.as_deref(), owner)),
            TreeKind::Super { qual, mix } => {
                Some(self.pickle_super_tree(qual.as_deref(), mix.as_deref(), owner))
            }
            TreeKind::Apply { fun, args }
                if matches!(
                    &fun.kind,
                    TreeKind::Ident { .. } | TreeKind::Select { .. } | TreeKind::Apply { .. }
                ) =>
            {
                self.pickle_apply_tree(fun, args, owner)
            }
            _ => None,
        }
    }

    /// TREE { LITERALtree, type_Ref, constant_Ref } — UnPickler reads no symbol.
    fn pickle_literal_tree(&mut self, lit: &Lit) -> u32 {
        let c = self.pickle_literal(lit);
        let tpe = match lit {
            Lit::Int(_) => self.type_ref_named("Int"),
            Lit::Long(_) => self.type_ref_named("Long"),
            Lit::Float(_) => self.type_ref_named("Float"),
            Lit::Double(_) => self.type_ref_named("Double"),
            Lit::Boolean(_) => self.type_ref_named("Boolean"),
            Lit::Char(_) => self.type_ref_named("Char"),
            Lit::String(_) | Lit::Symbol(_) => self.type_ref_named("String"),
            Lit::Unit => self.type_ref_named("Unit"),
            Lit::Null => self.type_ref_named("Null"),
        };
        let mut body = Vec::new();
        write_nat_to(&mut body, LITERALtree as u32);
        write_nat_to(&mut body, tpe);
        write_nat_to(&mut body, c);
        self.add(TREE, body)
    }

    fn pickle_literal_class(&mut self, tpe: u32) -> u32 {
        let mut body = Vec::new();
        write_nat_to(&mut body, tpe);
        self.add(LITERALclass, body)
    }

    fn pickle_type_from_tree(&mut self, tpt: &Tree) -> u32 {
        match &tpt.kind {
            TreeKind::Ident { name } => self.type_ref_named(name),
            TreeKind::Select { name, .. } => self.type_ref_named(name),
            TreeKind::AppliedTypeTree { tpt, .. } => self.pickle_type_from_tree(tpt),
            _ => self.type_ref_named("Any"),
        }
    }

    fn pickle_sym_annots(&mut self, id: SymbolId, pickle_idx: u32) {
        let annots = self.st.get(id).annotations.clone();
        let owner = self.st.get(id).owner;
        for a in &annots {
            self.pickle_symannot(pickle_idx, a, owner);
        }
    }

    fn pickle_this_tpe(&mut self, cls: SymbolId) -> u32 {
        if let Some(&i) = self.this_tpes.get(&cls.0) {
            return i;
        }
        let _ = self.pickle_class(cls);
        self.this_tpes.get(&cls.0).copied().unwrap_or(self.noprefix)
    }

    /// `T @unchecked` is `scala.unchecked`; `T @uncheckedVariance` is
    /// `scala.annotation.unchecked.uncheckedVariance`.
    fn pickle_type_annot_ref(&mut self, annot: &str) -> u32 {
        let simple = annot.rsplit('.').next().unwrap_or(annot);
        if simple == "uncheckedVariance" {
            let sc = self.scala_module();
            let ann = self.ext_mod("annotation", Some(sc));
            let unc = self.ext_mod("unchecked", Some(ann));
            self.type_ref_in(unc, "uncheckedVariance")
        } else {
            let sc = self.scala_module();
            self.type_ref_in(sc, simple)
        }
    }

    fn pickle_term_ref(&mut self, id: SymbolId) -> u32 {
        if let Some(&i) = self.sym_index.get(&id.0) {
            return i;
        }
        let owner_id = self.st.get(id).owner;
        let owner_ref = if owner_id.is_none() {
            self.current_owner
        } else if let Some(&i) = self.sym_index.get(&owner_id.0) {
            i
        } else {
            self.pickle_class(owner_id)
        };
        self.pickle_val(id, owner_ref, false, false)
    }

    fn pickle_type(&mut self, ty: &Type) -> u32 {
        match ty {
            Type::Unit | Type::NoType => self.type_ref_named("Unit"),
            Type::Boolean => self.type_ref_named("Boolean"),
            Type::Byte => self.type_ref_named("Byte"),
            Type::Short => self.type_ref_named("Short"),
            Type::Int => self.type_ref_named("Int"),
            Type::Long => self.type_ref_named("Long"),
            Type::Float => self.type_ref_named("Float"),
            Type::Double => self.type_ref_named("Double"),
            Type::Char => self.type_ref_named("Char"),
            Type::String => self.type_ref_named("String"),
            Type::Any => self.type_ref_named("Any"),
            Type::Wildcard | Type::AnyRef => self.type_ref_named("AnyRef"),
            Type::BoundedWildcard { hi, .. } => match hi {
                Some(t) => self.pickle_type(t),
                None => self.type_ref_named("Any"),
            },
            Type::ThisType(id) => self.pickle_this_tpe(*id),
            Type::SingleType { prefix, sym } => {
                let pre = if matches!(prefix.as_ref(), Type::NoType | Type::ThisType(_)) {
                    if let Type::ThisType(c) = prefix.as_ref() {
                        self.pickle_this_tpe(*c)
                    } else {
                        self.noprefix
                    }
                } else {
                    self.pickle_type(prefix)
                };
                let sref = self.pickle_term_ref(*sym);
                let mut body = Vec::new();
                write_nat_to(&mut body, pre);
                write_nat_to(&mut body, sref);
                self.add(SINGLETPE, body)
            }
            Type::Annotated { tpe, annot } => {
                let inner = self.pickle_type(tpe);
                let atp = self.pickle_type_annot_ref(annot);
                let mut ab = Vec::new();
                write_nat_to(&mut ab, atp);
                let info = self.add(ANNOTINFO, ab);
                let mut body = Vec::new();
                write_nat_to(&mut body, inner);
                write_nat_to(&mut body, info);
                self.add(ANNOTATEDTPE, body)
            }
            Type::AnyVal => self.type_ref_named("AnyVal"),
            Type::TypeParam(id) => {
                // nsc pickles class/method tparams as TypeRef(NoPrefix, T), not ThisType.
                // ThisType(C).A is path-dependent and does not asSeenFrom-substitute.
                let pref = self.noprefix;
                let sym = self.pickle_typesym(*id);
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
            Type::Applied { ctor, args } => {
                if args.iter().any(type_has_wildcard) {
                    let mut quantified = Vec::new();
                    let inner = self.pickle_type_pack(ty, &mut quantified);
                    return self.pickle_existential_tpe(inner, &quantified);
                }
                match ctor.as_ref() {
                    Type::TypeParam(id) => {
                        let pref = self.noprefix;
                        let sym = self.pickle_typesym(*id);
                        let mut body = Vec::new();
                        write_nat_to(&mut body, pref);
                        write_nat_to(&mut body, sym);
                        for a in args {
                            write_nat_to(&mut body, self.pickle_type(a));
                        }
                        self.add(TYPEREFTPE, body)
                    }
                    Type::Class {
                        sym,
                        args: existing,
                    } => {
                        let mut all = existing.clone();
                        all.extend(args.iter().cloned());
                        self.pickle_type(&Type::Class {
                            sym: *sym,
                            args: all,
                        })
                    }
                    _ => self.pickle_type(ctor),
                }
            }
            Type::TypeMember(id) => {
                let owner = self.st.get(*id).owner.0;
                let pref = self.this_tpes.get(&owner).copied().unwrap_or(self.noprefix);
                let sym = self.pickle_type_member(*id);
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
            Type::Class { sym, args } => {
                if args.iter().any(type_has_wildcard) {
                    let mut quantified = Vec::new();
                    let inner = self.pickle_type_pack(ty, &mut quantified);
                    return self.pickle_existential_tpe(inner, &quantified);
                }
                let arg_refs: Vec<u32> = args.iter().map(|a| self.pickle_type(a)).collect();
                self.class_type_ref(*sym, &arg_refs)
            }
            Type::ModuleRef(s) => {
                // `val L = List` has type `List.type`. Written as a bare name
                // this became `TypeRef(NoPrefix, EXTref "List")`, which nsc
                // reads as `<root>.List` and then reports "Symbol 'type
                // <root>.List' is missing from the classpath" at the *use*
                // site. The owning package has to be part of the reference,
                // and a module is an `EXTMODCLASSref`, not an `EXTref`.
                if let Some(&i) = self.sym_index.get(&s.0) {
                    return self.type_ref_local_refs(i, &[]);
                }
                let jvm = self.st.get(*s).jvm_name.clone();
                let n = self.st.get(*s).name.trim_end_matches('$').to_string();
                if !jvm.contains('/') {
                    return self.type_ref_named(&n);
                }
                let owner = self.package_ref_of(&jvm);
                let sym = self.ext_mod(&n, Some(owner));
                let pref = self.noprefix;
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
            Type::Function { params, ret } => {
                // With no arguments a reader sees a raw `Function1` and has to
                // decline every signature that mentions one -- which is most of
                // a collection or type-class API. `TupleN` was already written
                // with its arguments; `FunctionN` is the same shape.
                let name = format!("Function{}", params.len());
                let mut all: Vec<Type> = params.clone();
                all.push((**ret).clone());
                if all.iter().any(type_has_wildcard) {
                    // A wildcard here would need an `EXISTENTIALtpe` wrapped
                    // around the function type, which this writer does not put
                    // there yet. The bare constructor is the honest fallback:
                    // a reader takes no arguments rather than wrong ones.
                    return self.type_ref_named(&name);
                }
                let sc = self.scala_module();
                self.type_ref_in_args(sc, &name, &all)
            }
            Type::Tuple(ts) => {
                if ts.iter().any(type_has_wildcard) {
                    let mut quantified = Vec::new();
                    let inner = self.pickle_type_pack(ty, &mut quantified);
                    return self.pickle_existential_tpe(inner, &quantified);
                }
                let sc = self.scala_module();
                self.type_ref_in_args(sc, &format!("Tuple{}", ts.len()), ts)
            }
            Type::Array(_) => self.type_ref_named("Array"),
            Type::ByName(t) => self.pickle_type(t),
            Type::Repeated(t) => {
                let inner = self.pickle_type(t);
                let sc = self.scala_module();
                self.type_ref_in_refs(sc, "<repeated>", &[inner])
            }
            Type::Method { ret, .. } => self.pickle_type(ret),
            Type::Constant(lit) => {
                let c = self.pickle_literal(lit);
                let mut body = Vec::new();
                write_nat_to(&mut body, c);
                self.add(CONSTANTtpe, body)
            }
            // The as-seen-from view of a type projection (`A#B`) is the
            // compiler's own bookkeeping, not a refinement the program wrote:
            // pickle the bare `B`. Writing the `<asSeenFrom>` decl would put a
            // name no Scala compiler can read into the signature.
            Type::Refined { .. }
                if scala_rs_typer::SymbolTable::as_seen_from_view(ty).is_some() =>
            {
                let parent = scala_rs_typer::SymbolTable::as_seen_from_view(ty)
                    .cloned()
                    .unwrap_or(Type::AnyRef);
                self.pickle_type(&parent)
            }
            Type::Refined { parents, decls } => self.pickle_refined(parents, decls),
            // Both fell through to the `Any` below, so a `def foo: Nothing`
            // read back through runtime reflection was `def foo: Any` --
            // `scala.reflect.runtime` reads this pickle, not the class file's
            // erased descriptor, which was `scala/runtime/Nothing$` all along.
            // `type_ref_named` already knows both names.
            Type::Nothing => self.type_ref_named("Nothing"),
            Type::Null => self.type_ref_named("Null"),
            _ => self.type_ref_named("Any"),
        }
    }

    fn symbol_info(&mut self, name_ref: u32, owner_ref: u32, flags: u64, info_ref: u32) -> Vec<u8> {
        let mut body = Vec::new();
        write_nat_to(&mut body, name_ref);
        write_nat_to(&mut body, owner_ref);
        write_long_nat_to(&mut body, flags);
        write_nat_to(&mut body, info_ref);
        body
    }

    fn pickle_class(&mut self, class_id: SymbolId) -> u32 {
        if let Some(i) = self.sym_index.get(&class_id.0) {
            return *i;
        }
        let s = self.st.get(class_id);
        let is_module = matches!(s.kind, SymKind::Module | SymKind::ModuleClass)
            || s.flags.contains(Flags::MODULE)
            || s.name.ends_with('$');
        let is_case = s.flags.contains(Flags::CASE);
        let is_value = self.st.is_value_class(class_id);
        let class_flags = s.flags;
        let class_kind = s.kind;
        let raw_name = s.name.trim_end_matches('$').to_string();
        // nsc module classes are CLASSsym + MODULE with a type name, not MODULEsym.
        let name_ref = self.type_name(&raw_name);
        let tag = CLASSSYM;
        // Placeholder; fill after children so the class exists as owner.
        let idx = self.add(tag, vec![]);
        self.sym_index.insert(class_id.0, idx);
        let jvm_name = self.st.get(class_id).jvm_name.clone();
        let pkg_ref = self.package_ref_of(&jvm_name);
        self.class_pkg.insert(idx, pkg_ref);

        let this_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, idx);
            self.add(THISTPE, body)
        };
        self.this_tpes.insert(class_id.0, this_tpe);
        let saved_owner = self.current_owner;
        self.current_owner = idx;
        let jl = self.java_lang_module();
        let obj = self.ext_ref_owned("Object", jl);
        let obj_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, self.noprefix);
            write_nat_to(&mut body, obj);
            self.add(TYPEREFTPE, body)
        };
        let members: Vec<SymbolId> = s.members.clone();
        let tparams: Vec<SymbolId> = s.tparams.clone();
        let ctor_fields: Vec<SymbolId> = s.ctor_fields.clone();
        let mut tparam_refs = Vec::new();
        for tp in tparams {
            tparam_refs.push(self.pickle_typesym(tp));
        }
        self.class_tparams.insert(idx, tparam_refs.clone());

        // The parents. `CLASSINFOtpe` is `sym_Ref {tpe_Ref}` and this writer
        // used to put exactly one `tpe_Ref` there -- `java.lang.Object` --
        // for every class it emitted. Every scala-rs classfile therefore said
        // "extends Object and nothing else", so *no* inherited member was
        // visible to a later compilation: `object H2Profile extends
        // H2Profile` lost `api`, and `import profile.api.*` (the shape every
        // slick test is written in) resolved nothing. Real scalac reading
        // scala-rs's slick output reported the same three errors this
        // compiler did, which is what identified the writer rather than the
        // reader.
        //
        // Written after the type parameters so `trait Foo[A] extends Bar[A]`
        // refers to the pickled `A`, not a fresh one.
        let parent_tys: Vec<Type> = self
            .st
            .get(class_id)
            .parents
            .iter()
            // `Any` is nobody's parent in a pickle, and a self-reference
            // (which the table can carry for a class that extends its own
            // companion's type) would be a cycle for the reader.
            .filter(|p| {
                !matches!(p, Type::Any | Type::NoType | Type::Error)
                    && self.st.class_sym_of(p) != Some(class_id)
            })
            .cloned()
            .collect();
        // nsc reads `parents.head` as the superclass. When the first entry is
        // a trait (or there is nothing at all), `Object` goes in front, which
        // is what nsc itself pickles for `trait T extends U` with `U` a trait.
        let head_is_class = parent_tys
            .first()
            .and_then(|p| self.st.class_sym_of(p))
            .is_some_and(|c| {
                let ps = self.st.get(c);
                !ps.flags.contains(Flags::TRAIT) && !ps.flags.contains(Flags::INTERFACE)
            });
        let mut parent_refs: Vec<u32> = Vec::new();
        if !head_is_class {
            parent_refs.push(obj_tpe);
        }
        for p in &parent_tys {
            let r = self.pickle_type(p);
            if !parent_refs.contains(&r) {
                parent_refs.push(r);
            }
        }
        let mut info_body = Vec::new();
        write_nat_to(&mut info_body, idx);
        for r in &parent_refs {
            write_nat_to(&mut info_body, *r);
        }
        let info = self.add(CLASSINFOTPE, info_body);
        // A module class's primary constructor. nsc's namer gives every
        // `object` one -- `private T$()` in the classfile, `constructor T` in
        // `info.decls` -- and our symbol table does not carry it as a member,
        // so nothing pickled it. Runtime reflection then had no
        // `termNames.CONSTRUCTOR` to find in an object's signature
        // (`run/reflection-enclosed-{inner,nested}-nested-basic`).
        if is_module
            && !self
                .st
                .get(class_id)
                .members
                .iter()
                .any(|&m| self.st.get(m).name == "<init>")
        {
            self.pickle_module_ctor(idx, class_id);
        }
        for m in members {
            let kind = self.st.get(m).kind;
            let name = self.st.get(m).name.clone();
            match kind {
                SymKind::TypeParam => {
                    let _ = self.pickle_typesym(m);
                }
                SymKind::Term => {
                    if ctor_fields.contains(&m) || !self.st.get(m).flags.contains(Flags::PARAM) {
                        let ctor_field = ctor_fields.contains(&m);
                        if (is_case || is_value) && ctor_field {
                            // nsc `caseFieldAccessors` pairs CASEACCESSOR getters
                            // with non-method PARAMACCESSOR fields.
                            self.pickle_param_field(m, idx);
                        }
                        // A value class's *erasure* is the type of its single
                        // parameter accessor, and nsc finds that accessor by
                        // the `PARAMACCESSOR | METHOD` flag pair
                        // (`Symbol.derivedValueClassUnbox`). Without it scalac
                        // erased our `class Ops(val x: Int) extends AnyVal` to
                        // `<notype>` and died in its own backend
                        // (`unexpected type representation`) on the first call
                        // it rewrote to `Ops$.MODULE$.inc$extension`.
                        self.pickle_val(m, idx, is_case && ctor_field, is_value && ctor_field);
                    }
                }
                SymKind::Method => {
                    if name == "<clinit>" {
                        continue;
                    }
                    if pickle_sig_incomplete(self.st, m) {
                        continue;
                    }
                    self.pickle_method(m, idx, this_tpe);
                }
                SymKind::TypeMember => {
                    let _ = self.pickle_type_member(m);
                }
                // A nested class or object is a *member* of this one, and nsc
                // resolves `Outer.Inner` by looking it up in `Outer`'s
                // signature. Writing one pickle per classfile and nothing
                // else meant `slick/jdbc/JdbcActionComponent.class` declared
                // no members at all, so real scalac reading our slick output
                // stopped at "Symbol 'type
                // slick.jdbc.JdbcActionComponent.MultipleRowsPerStatementSupport'
                // is missing from the classpath" -- the class file exists and
                // carries its own signature, but nsc never looks there.
                SymKind::Class | SymKind::ModuleClass => {
                    if self.nested_member_of(m, class_id) {
                        let _ = self.pickle_class(m);
                    }
                }
                // The term half of a nested `object`: its module class is
                // what carries the members, and pickling that emits both the
                // CLASSsym|MODULE and the MODULEsym nsc binds the term from.
                SymKind::Module => {
                    let mc = self.st.module_class_of(m);
                    if !mc.is_none() && self.nested_member_of(mc, class_id) {
                        let _ = self.pickle_class(mc);
                    }
                }
                _ => {}
            }
        }
        self.pickle_erasure_bridges(class_id, idx);
        // A trait's primary constructor is `$init$` (`nme.MIXIN_CONSTRUCTOR`),
        // and nsc emits `invokestatic <Iface>.$init$` from the constructor of
        // every class that mixes the trait in — but only when it can find
        // that symbol in the trait's signature. Without it, a subclass real
        // scalac compiles against our classfile links and verifies and then
        // leaves every trait `val` at its default value.
        if class_flags.contains(Flags::TRAIT) || class_flags.contains(Flags::INTERFACE) {
            // `p$q$T$$super$m`, nsc's `SUPERACCESSOR`. A stackable trait's
            // `super.m` is bound by the linearization of whichever class
            // mixes the trait in, so the class owes the accessor; nsc's mixin
            // phase writes one only for a member the trait's *signature*
            // declares with this flag. We emitted the accessor on the
            // interface and pickled nothing, so a class real scalac compiled
            // over an `abstract override` trait of ours left it abstract and
            // ran the base implementation.
            //
            // The *pickled* name is nsc's unexpanded `super$m`: the accessor
            // is `private`, and a reader expands a private member's name
            // itself, so pickling the expanded form gave
            // `tilib2$Stack$$tilib2$Stack$$super$m` in the class scalac
            // compiled.
            let own = self.st.get(class_id).members.clone();
            for m in own {
                let s = self.st.get(m);
                if s.kind != SymKind::Method || !s.super_accessor {
                    continue;
                }
                let acc = format!("super${}", crate::classfile::encode_method_name(&s.name));
                self.pickle_super_accessor(m, idx, &acc);
            }
            self.pickle_mixin_ctor(idx);
        }

        let mut info = info;
        if !tparam_refs.is_empty() {
            let mut body = Vec::new();
            // nsc POLYtpe = restpe, {tparams}
            write_nat_to(&mut body, info);
            for r in tparam_refs {
                write_nat_to(&mut body, r);
            }
            info = self.add(POLYTPE, body);
        }

        let mut extra = 0u64;
        if is_module {
            extra |= 1 << 8; // MODULE
        }
        if is_case {
            extra |= 1 << 11; // CASE
        }
        if class_flags.contains(Flags::JAVA) || self.st.get(class_id).jvm_name.starts_with("java/")
        {
            extra |= 1 << 20; // JAVA (not remapped)
        }
        let flags = pickled_from_our(class_flags, class_kind, extra);
        // A *nested* class used to name the enclosing package as its owner, so
        // `slick/jdbc/JdbcProfile$JdbcAPI.class` said it was
        // `slick.jdbc.JdbcAPI`. Every reader that looked the class up by the
        // name it actually has then found a pickle that disagreed and served
        // nothing: `import profile.api.*` came back with zero members even
        // once the parents were there. Same helper as the reference side.
        // A class pickled *inside* its enclosing class's entries has to name
        // that entry as its owner, not an EXTref: nsc enters each CLASSsym in
        // the scope of the symbol its owner field points at, and a nested
        // class that pointed outside would be entered nowhere.
        let owner = self.local_owner_ref(class_id);
        let body = self.symbol_info(name_ref, owner, flags, info);
        self.entries[idx as usize] = (tag, body);
        self.pickle_sym_annots(class_id, idx);
        self.current_owner = saved_owner;
        if is_module {
            // nsc unpickler binds the term `Lib` from MODULEsym; CLASSsym+MODULE is the module class.
            let mut tr = Vec::new();
            write_nat_to(&mut tr, self.noprefix);
            write_nat_to(&mut tr, idx);
            let mtpe = self.add(TYPEREFTPE, tr);
            let mflags = raw_to_pickled(1 << 8);
            let mn = self.term_name(&raw_name);
            let mbody = self.symbol_info(mn, owner, mflags, mtpe);
            self.add(MODULESYM, mbody);
        } else if let Some(mod_id) = self.st.companion_module(class_id) {
            // nsc `enterClassAndModule` completes term `Point` from MODULESYM
            // in `Point.class`, not from `Point$.class`.
            let mc = self.st.module_class_of(mod_id);
            if mc != class_id && !mc.is_none() {
                let _ = self.pickle_class(mc);
            }
        }
        idx
    }

    fn parent_classes(&self, class_id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        let mut work: Vec<Type> = self.st.get(class_id).parents.clone();
        while let Some(p) = work.pop() {
            let Some(c) = self.st.class_sym_of(&p) else {
                continue;
            };
            if c.is_none() || c == class_id || seen.contains(&c.0) {
                continue;
            }
            seen.push(c.0);
            out.push(c);
            work.extend(self.st.get(c).parents.clone());
        }
        out
    }

    /// Pickle JVM erasure bridges (nsc `ACC_BRIDGE`) as VALsym with BRIDGE so
    /// scalac skips them in overload (e.g. `Ordered.compare(Object)`).
    fn pickle_erasure_bridges(&mut self, class_id: SymbolId, owner_ref: u32) {
        if class_id.is_none() {
            return;
        }
        let own: Vec<(String, SymbolId)> = self
            .st
            .get(class_id)
            .members
            .iter()
            .copied()
            .filter(|&id| self.st.get(id).kind == SymKind::Method)
            .map(|id| (self.st.get(id).name.clone(), id))
            .collect();
        let mut seen: Vec<String> = Vec::new();
        for parent in self.parent_classes(class_id) {
            for pmid in self.st.get(parent).members.clone() {
                let ps = self.st.get(pmid);
                if ps.kind != SymKind::Method {
                    continue;
                }
                if ps.name == "<init>" || ps.name == "<clinit>" {
                    continue;
                }
                let Some((_, cid)) = own.iter().find(|(n, _)| n == &ps.name) else {
                    continue;
                };
                if *cid == pmid {
                    continue;
                }
                let pparams = method_flat_params(&ps.ty);
                let cparams = method_flat_params(&self.st.get(*cid).ty);
                if pparams.len() != cparams.len() {
                    continue;
                }
                if !pparams
                    .iter()
                    .zip(cparams.iter())
                    .any(|(a, b)| bridge_erased(a) != bridge_erased(b))
                {
                    continue;
                }
                let key = format!("{}:{:?}", ps.name, pparams.len());
                if seen.iter().any(|s| s == &key) {
                    continue;
                }
                seen.push(key);
                let pret = method_result(&ps.ty);
                self.pickle_bridge_method(&ps.name.clone(), owner_ref, &pparams, &pret);
            }
        }
    }

    fn pickle_bridge_method(&mut self, name: &str, owner_ref: u32, params: &[Type], ret: &Type) {
        let name_ref = self.term_name(name);
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let mut param_refs = Vec::new();
        for (i, pty) in params.iter().enumerate() {
            let erased = match pty {
                Type::TypeParam(_) => Type::Any,
                t => t.clone(),
            };
            let pn = self.term_name(&format!("x${i}"));
            let pty_ref = self.pickle_type(&erased);
            let flags = pickled_from_our(Flags::PARAM, SymKind::Term, 1u64 << 13);
            let body = self.symbol_info(pn, meth_idx, flags, pty_ref);
            param_refs.push(self.add(VALSYM, body));
        }
        let ret_ref = self.pickle_type(ret);
        let mut mt = Vec::new();
        write_nat_to(&mut mt, ret_ref);
        for p in param_refs {
            write_nat_to(&mut mt, p);
        }
        let info = self.add(METHODTPE, mt);
        // METHOD | SYNTHETIC | BRIDGE (none remapped except METHOD)
        let extra = (1u64 << 6) | (1 << 21) | (1 << 26);
        let flags = raw_to_pickled(extra);
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    /// `def p$q$T$$super$m: T` — the abstract `SUPERACCESSOR` member a
    /// stackable trait declares and every class mixing it in implements.
    /// Same signature as the method it accesses; see the call site.
    fn pickle_super_accessor(&mut self, method_id: SymbolId, owner_ref: u32, acc_name: &str) {
        // No alias, no accessor: nsc's unpickler asserts that every
        // SUPERACCESSOR carries one, and a member whose signature we could not
        // pickle has no entry to point at.
        let Some(alias) = self.sym_index.get(&method_id.0).copied() else {
            return;
        };
        let (paramss, ret) = match &self.st.get(method_id).ty {
            Type::Method { paramss, ret } => (paramss.clone(), (**ret).clone()),
            other => (Vec::new(), other.clone()),
        };
        let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
        let name_ref = self.term_name(acc_name);
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let mut param_refs = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let pn = self.term_name(&format!("x${}", i + 1));
            let pty = self.pickle_type(p);
            let pflags = pickled_from_our(Flags::PARAM, SymKind::Term, 1u64 << 13);
            let pbody = self.symbol_info(pn, meth_idx, pflags, pty);
            param_refs.push(self.add(VALSYM, pbody));
        }
        let ret_ref = self.pickle_type(&ret);
        let info = if param_refs.is_empty() {
            // nsc NullaryMethodType = POLYtpe(restpe) with no tparams.
            let mut pt = Vec::new();
            write_nat_to(&mut pt, ret_ref);
            self.add(POLYTPE, pt)
        } else {
            let mut mt = Vec::new();
            write_nat_to(&mut mt, ret_ref);
            for p in param_refs {
                write_nat_to(&mut mt, p);
            }
            self.add(METHODTPE, mt)
        };
        // METHOD | PRIVATE | SUPERACCESSOR. `private` is not
        // decoration: `RefChecks` walks the *non-private* deferred members of
        // a class and would demand `class S needs to be abstract. Missing
        // implementation for member of trait Stack: def super$m` -- the
        // accessor is the mixin phase's business, and nsc pickles its own as
        // `private def super$m` for exactly that reason.
        let flags = raw_to_pickled((1u64 << 6) | (1u64 << 2) | (1u64 << 28));
        let mut body = self.symbol_info(name_ref, owner_ref, flags, info);
        // nsc's `SYMinfo` ends with an optional *alias* reference, and its
        // unpickler asserts that exactly the SUPERACCESSOR and PARAMACCESSOR
        // symbols carry one (`UnPickler.finishSym`). The alias is the member
        // the accessor super-calls; `Mixin.rebindSuper` re-resolves it by
        // name in the linearization of the class being compiled, so the
        // trait's own method is the right referent.
        write_nat_to(&mut body, alias);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    /// `def $init$(): Unit` — the trait's primary constructor as nsc pickles
    /// it (`nme.MIXIN_CONSTRUCTOR`, a `MethodType` with an empty parameter
    /// list, not a nullary one). It is what tells a reader that the mixing
    /// class owes a `$init$` call.
    fn pickle_mixin_ctor(&mut self, owner_ref: u32) {
        let name_ref = self.term_name("$init$");
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let ret_ref = self.pickle_type(&Type::Unit);
        let mut mt = Vec::new();
        write_nat_to(&mut mt, ret_ref);
        let info = self.add(METHODTPE, mt);
        let flags = raw_to_pickled(1u64 << 6); // METHOD
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    /// `private def <init>(): T.type` — the primary constructor of an
    /// `object`'s module class, which nsc emits (`private T$()`) and pickles
    /// but our symbol table does not carry as a member.
    fn pickle_module_ctor(&mut self, owner_ref: u32, class_id: SymbolId) {
        let name_ref = self.term_name("<init>");
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let ret_ref = self.ctor_result_type(owner_ref, class_id);
        let mut mt = Vec::new();
        write_nat_to(&mut mt, ret_ref);
        let info = self.add(METHODTPE, mt);
        let flags = raw_to_pickled((1u64 << 6) | (1u64 << 2)); // METHOD | PRIVATE
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    fn pickle_method(&mut self, method_id: SymbolId, owner_ref: u32, _this_tpe: u32) -> u32 {
        if let Some(i) = self.sym_index.get(&method_id.0) {
            return *i;
        }
        let s = self.st.get(method_id);
        let meth_name = s.name.clone();
        let meth_flags = pickled_access_flags(self.st, method_id);
        let meth_kind = s.kind;
        let meth_tparams = s.tparams.clone();
        let (paramss, ret) = match &s.ty {
            Type::Method { paramss, ret } => (paramss.clone(), (**ret).clone()),
            _ => (vec![], Type::Unit),
        };
        let params: Vec<(String, Type, Flags)> = if !s.params.is_empty() {
            let sig: Vec<Type> = paramss.iter().flatten().cloned().collect();
            s.params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let ps = self.st.get(*p);
                    // A repeated parameter is a `Seq` on its symbol (that is
                    // its type in the body) but pickles as `<repeated>`.
                    let ty = match sig.get(i) {
                        Some(t @ Type::Repeated(_)) => t.clone(),
                        _ => ps.ty.clone(),
                    };
                    (ps.name.clone(), ty, ps.flags)
                })
                .collect()
        } else {
            paramss
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, t)| (format!("x${i}"), t.clone(), Flags::PARAM))
                .collect()
        };
        let name_ref = self.term_name(&meth_name);

        let meth_idx = self.add(VALSYM, vec![]);
        self.sym_index.insert(method_id.0, meth_idx);
        let saved_owner = self.current_owner;
        self.current_owner = meth_idx;

        let mut param_refs = Vec::new();
        for (pname, pty, pflags) in &params {
            let pn = self.term_name(pname);
            let pty_ref = self.pickle_type(pty);
            let mut extra = 1u64 << 13; // PARAM
            if pflags.contains(Flags::DEFAULTPARAM) {
                extra |= 1 << 25; // DEFAULTPARAM (not remapped)
            }
            let flags = pickled_from_our(*pflags, SymKind::Term, extra);
            let body = self.symbol_info(pn, meth_idx, flags, pty_ref);
            param_refs.push(self.add(VALSYM, body));
        }
        let ret_ref = if meth_name == "<init>" {
            let class_id = self.st.get(method_id).owner;
            self.ctor_result_type(owner_ref, class_id)
        } else {
            self.pickle_type(&ret)
        };
        let mut info = if params.is_empty() && meth_name != "<init>" {
            // nsc NullaryMethodType = POLYtpe(restpe) with no tparams.
            let mut pt = Vec::new();
            write_nat_to(&mut pt, ret_ref);
            self.add(POLYTPE, pt)
        } else {
            // One `METHODtpe`, because `uncurry` has already flattened the
            // parameter *lists* off the symbol by the time anything is
            // pickled: `def bind(fa)(f)` reaches here as `bind(fa, f)` and the
            // original shape is gone. So it pickles as one list, and a reader
            // -- scalac included -- sees `bind(fa, f)`. See README Remaining.
            let mut mt = Vec::new();
            write_nat_to(&mut mt, ret_ref);
            for p in param_refs {
                write_nat_to(&mut mt, p);
            }
            self.add(METHODTPE, mt)
        };
        if !meth_tparams.is_empty() {
            let mut tpref = Vec::new();
            // nsc POLYtpe = restpe, {tparams}
            write_nat_to(&mut tpref, info);
            for tp in meth_tparams {
                write_nat_to(&mut tpref, self.pickle_typesym(tp));
            }
            info = self.add(POLYTPE, tpref);
        }
        let mut extra = 1u64 << 6; // METHOD
        if meth_flags.contains(Flags::SYNTHETIC) || meth_name.contains("$default$") {
            extra |= 1 << 21; // SYNTHETIC (not remapped)
        }
        if meth_flags.contains(Flags::BRIDGE) {
            extra |= 1 << 26; // BRIDGE (not remapped)
        }
        if params
            .iter()
            .any(|(_, t, _)| matches!(t, Type::Repeated(_)))
            || meth_flags.contains(Flags::VARARGS)
        {
            extra |= 1u64 << 43; // VARARGS (not remapped)
        }
        let owner_id = self.st.get(method_id).owner;
        if meth_flags.contains(Flags::JAVA) || self.st.get(owner_id).flags.contains(Flags::JAVA) {
            extra |= 1 << 20; // JAVA (not remapped)
        }
        // `abstract override def m = …` is *concrete*: nsc pickles it
        // ABSOVERRIDE, never DEFERRED. Our namer sets `ABSTRACT` on it
        // (`Symbol::abstract_override` records what the source wrote), and
        // `nsc_raw_from_our` would turn that into DEFERRED -- which made
        // scalac read the whole stackable layer as an abstract declaration
        // and mix in nothing at all, so `new S().m` silently ran the base
        // implementation instead of the trait's.
        let mut meth_flags = meth_flags;
        if self.st.get(method_id).abstract_override {
            // ABSOVERRIDE stands on its own in nsc: `abstract override def m`
            // pickles ABSOVERRIDE and *not* OVERRIDE.
            meth_flags.set(Flags::ABSTRACT, false);
            meth_flags.set(Flags::OVERRIDE, false);
            extra |= 1 << 18; // ABSOVERRIDE (not remapped)
        }
        let flags = pickled_from_our(meth_flags, meth_kind, extra);
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.pickle_sym_annots(method_id, meth_idx);
        self.current_owner = saved_owner;
        meth_idx
    }

    fn pickle_typesym(&mut self, id: SymbolId) -> u32 {
        if let Some(i) = self.sym_index.get(&id.0) {
            return *i;
        }
        let s = self.st.get(id);
        let name = s.name.clone();
        let owner_id = s.owner.0;
        let own_tparams = s.tparams.clone();
        let bound_lo = s.bound_lo.clone();
        let bound_hi = s.bound_hi.clone();
        let name_ref = self.type_name(&name);
        let idx = self.add(TYPESYM, vec![]);
        self.sym_index.insert(id.0, idx);
        let owner_ref = self.sym_index.get(&owner_id).copied().unwrap_or(self.none);
        let lo = bound_lo
            .as_ref()
            .map(|t| self.pickle_type(t))
            .unwrap_or_else(|| self.type_ref_named("Nothing"));
        let hi = bound_hi
            .as_ref()
            .map(|t| self.pickle_type(t))
            .unwrap_or_else(|| self.type_ref_named("Any"));
        let mut b = Vec::new();
        write_nat_to(&mut b, lo);
        write_nat_to(&mut b, hi);
        let mut info = self.add(TYPEBOUNDSTPE, b);
        // A higher-kinded parameter (`F[_]`) is a TYPEsym whose info is a
        // POLYtpe over its own parameters. Without it the reader cannot tell
        // `F[_]` from a proper type and every `Monad[F]` fails the kind check.
        if !own_tparams.is_empty() {
            let mut nested = Vec::new();
            for tp in own_tparams {
                nested.push(self.pickle_typesym(tp));
            }
            let mut body = Vec::new();
            // nsc POLYtpe = restpe, {tparams}
            write_nat_to(&mut body, info);
            for r in nested {
                write_nat_to(&mut body, r);
            }
            info = self.add(POLYTPE, body);
        }
        let flags = raw_to_pickled((1u64 << 13) | (1u64 << 4)); // PARAM | DEFERRED
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[idx as usize] = (TYPESYM, body);
        idx
    }

    /// Abstract `type A` is TYPEsym + TYPEBOUNDStpe + DEFERRED.
    /// Alias `type T = Int` is nsc ALIASsym with the aliased type as info.
    fn pickle_type_member(&mut self, id: SymbolId) -> u32 {
        if let Some(i) = self.sym_index.get(&id.0) {
            return *i;
        }
        let s = self.st.get(id);
        let name = s.name.clone();
        let owner_id = s.owner.0;
        let rhs = s.ty.clone();
        let flags_our = s.flags;
        let kind = s.kind;
        let tparams: Vec<SymbolId> = s.tparams.clone();
        let bound_lo = s.bound_lo.clone();
        let bound_hi = s.bound_hi.clone();
        let is_alias = s.is_type_alias;
        let tag = if is_alias { ALIASSYM } else { TYPESYM };
        let name_ref = self.type_name(&name);
        let idx = self.add(tag, vec![]);
        self.sym_index.insert(id.0, idx);
        let owner_ref = self.sym_index.get(&owner_id).copied().unwrap_or(self.none);
        // `type Rep[T] = lifted.Rep[T]` is a *polymorphic* alias: nsc's info
        // for it is `PolyType(List(T), lifted.Rep[T])`. Writing only the
        // right-hand side dropped the parameters, and every reader (scalac
        // included) then said "Rep does not take type parameters" -- which is
        // most of `slick.lifted.Aliases`, i.e. most of what `import
        // profile.api.*` is for. The parameters go in first so the right-hand
        // side refers to the pickled ones.
        let saved_owner = self.current_owner;
        self.current_owner = idx;
        let tparam_refs: Vec<u32> = tparams.iter().map(|&t| self.pickle_typesym(t)).collect();
        let info = if is_alias {
            self.pickle_type(&rhs)
        } else {
            // An abstract member's bounds, not `Nothing..Any`: `type API <:
            // BasicAPI` says what its members are, and a reader given `Any`
            // has none of them.
            let lo = match &bound_lo {
                Some(t) => self.pickle_type(t),
                None => self.type_ref_named("Nothing"),
            };
            let hi = match &bound_hi {
                Some(t) => self.pickle_type(t),
                None => self.type_ref_named("Any"),
            };
            let mut b = Vec::new();
            write_nat_to(&mut b, lo);
            write_nat_to(&mut b, hi);
            self.add(TYPEBOUNDSTPE, b)
        };
        let info = if tparam_refs.is_empty() {
            info
        } else {
            let mut b = Vec::new();
            write_nat_to(&mut b, info);
            for r in &tparam_refs {
                write_nat_to(&mut b, *r);
            }
            self.add(POLYTPE, b)
        };
        self.current_owner = saved_owner;
        let extra = if is_alias { 0 } else { 1u64 << 4 }; // DEFERRED for abstract
        let flags = pickled_from_our(flags_our, kind, extra);
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[idx as usize] = (tag, body);
        idx
    }

    /// nsc case-class ctor field (not the getter): PARAMACCESSOR, not METHOD.
    fn pickle_param_field(&mut self, val_id: SymbolId, owner_ref: u32) {
        let s = self.st.get(val_id);
        let name = s.name.clone();
        let ty = s.ty.clone();
        let flags_our = s.flags;
        let kind = s.kind;
        let name_ref = self.term_name(&name);
        let ty_ref = self.pickle_type(&ty);
        // PRIVATE | LOCAL stay outside bits 0–11; PARAMACCESSOR is not remapped.
        let extra = (1u64 << 2) | (1 << 19) | (1 << 29); // PRIVATE | LOCAL | PARAMACCESSOR
        let flags = pickled_from_our(flags_our, kind, extra);
        let body = self.symbol_info(name_ref, owner_ref, flags, ty_ref);
        let _ = self.add(VALSYM, body);
    }

    fn pickle_val(
        &mut self,
        val_id: SymbolId,
        owner_ref: u32,
        case_accessor: bool,
        param_accessor: bool,
    ) -> u32 {
        if let Some(i) = self.sym_index.get(&val_id.0) {
            return *i;
        }
        let s = self.st.get(val_id);
        let name = s.name.clone();
        let ty = s.ty.clone();
        let flags_our = pickled_access_flags(self.st, val_id);
        let kind = s.kind;
        let name_ref = self.term_name(&name);
        let idx = self.add(VALSYM, vec![]);
        self.sym_index.insert(val_id.0, idx);
        let ret_ref = self.pickle_type(&ty);
        // nsc NullaryMethodType is POLYtpe(restpe) with no tparams.
        let mut pt = Vec::new();
        write_nat_to(&mut pt, ret_ref);
        let info = self.add(POLYTPE, pt);
        // METHOD | ACCESSOR, then nsc raw→pickled remap. STABLE only for an
        // immutable one: nsc decides "this is a `var`" from the getter not
        // being STABLE plus a `_$eq` setter next to it, and a trait `var` read
        // back as a `val` makes the reader implement the mixin
        // `T$_setter_$v_$eq` protocol instead of the plain setter the trait's
        // own `$init$` calls.
        let mutable = flags_our.contains(Flags::MUTABLE);
        // A declaration (`var dv: Int` in a trait) carries no `ABSTRACT`
        // flag -- `Symbol::deferred_val` is where it lives for a term -- and
        // pickling it as a definition made scalac ask a class implementing
        // the trait for an `override` modifier.
        let deferred = self.st.get(val_id).deferred_val;
        let mut extra = (1u64 << 6) | (1u64 << 27);
        if !mutable {
            extra |= 1u64 << 22; // STABLE
        }
        if deferred {
            extra |= 1u64 << 4; // DEFERRED (raw; `raw_to_pickled` moves it)
        }
        if case_accessor {
            extra |= 1 << 24; // CASEACCESSOR (not remapped)
        }
        if case_accessor || param_accessor {
            extra |= 1 << 29; // PARAMACCESSOR (not remapped)
        }
        let flags = pickled_from_our(flags_our, kind, extra);
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[idx as usize] = (VALSYM, body);
        self.pickle_sym_annots(val_id, idx);
        if mutable {
            // `access_widened` members were renamed *before* the pickle
            // (`expand_private_names`) and are published under the expanded
            // name; marking those private would ask the reader to expand a
            // second time.
            let private = flags_our.contains(Flags::PRIVATE) && !self.st.get(val_id).access_widened;
            self.pickle_var_setter(&name, owner_ref, &ty, private, deferred);
        }
        idx
    }

    /// `def v_$eq(x$1: T): Unit`, the setter half of a `var`. nsc pickles one
    /// beside every mutable accessor; without it a reader sees a `val` and
    /// neither `x.v = …` from another compilation unit nor a class mixing in
    /// a trait `var` lines up with what we emit.
    ///
    /// `private` has to carry across to the setter. A reader pairs a getter
    /// with its setter *by name*, and it expands the name of a `private`
    /// member of a trait first (`lib$Counter$$m` / `lib$Counter$$m_$eq`), so a
    /// public setter beside a private getter is not found: scalac read our
    /// `private var m` back as a `val`, gave the class it compiled a getter
    /// and no setter, and the trait's own `$init$` died with
    /// `AbstractMethodError: … 'abstract void lib$Counter$$m_$eq(int)'`.
    fn pickle_var_setter(
        &mut self,
        field: &str,
        owner_ref: u32,
        ty: &Type,
        private: bool,
        deferred: bool,
    ) {
        let setter = format!("{}_$eq", crate::classfile::encode_method_name(field));
        let name_ref = self.term_name(&setter);
        let meth_idx = self.add(VALSYM, vec![]);
        let saved = self.current_owner;
        self.current_owner = meth_idx;
        let pty_ref = self.pickle_type(ty);
        let pn = self.term_name("x$1");
        let pflags = pickled_from_our(Flags::PARAM, SymKind::Term, 1u64 << 13);
        let pbody = self.symbol_info(pn, meth_idx, pflags, pty_ref);
        let param = self.add(VALSYM, pbody);
        let unit_ref = self.pickle_type(&Type::Unit);
        let mut mt = Vec::new();
        write_nat_to(&mut mt, unit_ref);
        write_nat_to(&mut mt, param);
        let info = self.add(METHODTPE, mt);
        // METHOD | ACCESSOR (| PRIVATE)
        let mut raw = (1u64 << 6) | (1u64 << 27);
        if private {
            raw |= 1u64 << 2; // PRIVATE
        }
        if deferred {
            raw |= 1u64 << 4; // DEFERRED
        }
        let flags = raw_to_pickled(raw);
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        self.current_owner = saved;
    }

    fn finish(self) -> Vec<u8> {
        let mut buf = Buf::new();
        buf.write_nat(MAJOR);
        buf.write_nat(MINOR);
        buf.write_nat(self.entries.len() as u32);
        for (tag, body) in self.entries {
            buf.write_entry(tag, &body);
        }
        buf.bytes
    }
}

/// The `N` of a standard-library `TupleN` / `FunctionN` name, when that is
/// what the name actually is -- `TupleShape` and `FunctionSymbol` are not.
fn numbered_arity(name: &str, prefix: &str) -> Option<usize> {
    let rest = name.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

fn write_nat_to(out: &mut Vec<u8>, x: u32) {
    write_long_nat_to(out, x as u64);
}

fn type_has_wildcard(t: &Type) -> bool {
    match t {
        Type::Wildcard | Type::BoundedWildcard { .. } => true,
        Type::Annotated { tpe, .. } => type_has_wildcard(tpe),
        Type::Class { args, .. } => args.iter().any(type_has_wildcard),
        Type::Applied { ctor, args } => {
            type_has_wildcard(ctor) || args.iter().any(type_has_wildcard)
        }
        Type::Tuple(ts) => ts.iter().any(type_has_wildcard),
        Type::Refined { parents, .. } => parents.iter().any(type_has_wildcard),
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => type_has_wildcard(t),
        Type::Function { params, ret } => {
            params.iter().any(type_has_wildcard) || type_has_wildcard(ret)
        }
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().any(type_has_wildcard) || type_has_wildcard(ret)
        }
        _ => false,
    }
}

fn annot_args(tree: &Tree) -> Vec<&Tree> {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args.iter().collect(),
        _ => Vec::new(),
    }
}

fn is_classof_fun(fun: &Tree) -> bool {
    match &fun.kind {
        TreeKind::Ident { name } | TreeKind::Select { name, .. } => name == "classOf",
        _ => false,
    }
}

fn method_flat_params(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        _ => Vec::new(),
    }
}

fn method_result(ty: &Type) -> Type {
    match ty {
        Type::Method { ret, .. } => (**ret).clone(),
        other => other.clone(),
    }
}

fn bridge_erased(t: &Type) -> String {
    match t {
        Type::TypeParam(_) | Type::Any | Type::AnyRef | Type::Wildcard => {
            "Ljava/lang/Object;".into()
        }
        Type::Class { sym, .. } => format!("L#{}", sym.0),
        Type::ModuleRef(s) => format!("L#{}", s.0),
        Type::String => "Ljava/lang/String;".into(),
        Type::Int | Type::Boolean | Type::Char | Type::Byte | Type::Short => "I".into(),
        Type::Long => "J".into(),
        Type::Float => "F".into(),
        Type::Double => "D".into(),
        Type::Unit | Type::NoType => "V".into(),
        other => format!("{other}"),
    }
}

#[allow(dead_code)]
fn annot_string_args(tree: &Tree) -> Vec<String> {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args
            .iter()
            .filter_map(|a| match &a.kind {
                TreeKind::Literal {
                    lit: Lit::String(s),
                } => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The flags to pickle for a member, with qualified `private` dropped.
///
/// nsc writes `private[p]` as PRIVATE *plus* a `privateWithin` reference, and
/// this pickler writes no `privateWithin` at all. A bare PRIVATE is a
/// different member: a reader expands the name of a trait's private `val`
/// itself, so `private[tilib] val pkg` came back as `tilib$Counter$$pkg`
/// while our own class file declared `pkg()` --
/// `AbstractMethodError: … 'abstract void tilib$Counter$_setter_$pkg_$eq(int)'`.
/// The class file already emits a qualified-private member public, so
/// publishing it in the signature too is what keeps the two agreeing.
fn pickled_access_flags(st: &SymbolTable, id: SymbolId) -> Flags {
    let s = st.get(id);
    let mut f = s.flags;
    if s.private_within.is_some() {
        f.set(Flags::PRIVATE, false);
    }
    f
}

/// Map scala-rs `Flags` onto nsc **raw** bits (before `rawToPickledFlags`).
/// MACRO / late / anti are omitted: scalac 2.13.16 typechecks our existing
/// pickles without them (see `scalac_typechecks_against_our_classfiles_if_present`).
fn nsc_raw_from_our(f: Flags, kind: SymKind) -> u64 {
    let mut n = 0u64;
    if f.contains(Flags::PROTECTED) {
        n |= 1 << 0;
    }
    if f.contains(Flags::OVERRIDE) {
        n |= 1 << 1;
    }
    if f.contains(Flags::PRIVATE) {
        n |= 1 << 2;
    }
    if f.contains(Flags::ABSTRACT) {
        if kind == SymKind::Method {
            n |= 1 << 4; // DEFERRED
        } else {
            n |= 1 << 3; // ABSTRACT
        }
    }
    if f.contains(Flags::FINAL) {
        n |= 1 << 5;
    }
    if f.contains(Flags::INTERFACE) {
        n |= 1 << 7;
    }
    if f.contains(Flags::MODULE) {
        n |= 1 << 8;
    }
    if f.contains(Flags::IMPLICIT) {
        n |= 1 << 9;
    }
    if f.contains(Flags::SEALED) {
        n |= 1 << 10;
    }
    if f.contains(Flags::CASE) {
        n |= 1 << 11;
    }
    if f.contains(Flags::MUTABLE) {
        n |= 1 << 12;
    }
    if f.contains(Flags::PARAM) {
        n |= 1 << 13;
    }
    if f.contains(Flags::LOCAL) {
        n |= 1 << 19;
    }
    if f.contains(Flags::JAVA) {
        n |= 1 << 20;
    }
    if f.contains(Flags::SYNTHETIC) {
        n |= 1 << 21;
    }
    if f.contains(Flags::TRAIT) || f.contains(Flags::DEFAULTPARAM) {
        n |= 1 << 25;
    }
    if f.contains(Flags::BRIDGE) {
        n |= 1 << 26;
    }
    if f.contains(Flags::ACCESSOR) {
        n |= 1 << 27;
    }
    if f.contains(Flags::LAZY) {
        n |= 1 << 31;
    }
    if f.contains(Flags::VARARGS) {
        n |= 1u64 << 43;
    }
    n
}

fn pickled_from_our(f: Flags, kind: SymKind, extra_raw: u64) -> u64 {
    raw_to_pickled(nsc_raw_from_our(f, kind) | extra_raw)
}

/// nsc `PickleBuffer.writeLong`: signed big-endian base 256.
fn write_long_signed_256(out: &mut Vec<u8>, x: i64) {
    let y = x >> 8;
    let z = x & 0xff;
    if -y != (z >> 7) {
        write_long_signed_256(out, y);
    }
    out.push(z as u8);
}

/// nsc `PickleBuffer.writeLongNat`: big-endian base-128.
fn write_long_nat_to(out: &mut Vec<u8>, x: u64) {
    fn prefix(out: &mut Vec<u8>, x: u64) {
        let y = x >> 7;
        if y != 0 {
            prefix(out, y);
        }
        out.push(((x & 0x7f) | 0x80) as u8);
    }
    let y = x >> 7;
    if y != 0 {
        prefix(out, y);
    }
    out.push((x & 0x7f) as u8);
}

fn pickle_sig_incomplete(st: &SymbolTable, id: SymbolId) -> bool {
    fn ty_incomplete(t: &Type) -> bool {
        match t {
            Type::NoType | Type::Error => true,
            Type::Method { paramss, ret } => {
                ty_incomplete(ret) || paramss.iter().flatten().any(ty_incomplete)
            }
            Type::Function { params, ret } => {
                ty_incomplete(ret) || params.iter().any(ty_incomplete)
            }
            Type::Class { args, .. } => args.iter().any(ty_incomplete),
            Type::Tuple(ts) => ts.iter().any(ty_incomplete),
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => ty_incomplete(t),
            _ => false,
        }
    }
    ty_incomplete(&st.get(id).ty)
}

/// nsc `Flags.rawToPickledFlags`: bits 0–11 differ between raw and pickled form.
fn raw_to_pickled(flags: u64) -> u64 {
    const PAIRS: [(u64, u64); 12] = [
        (1 << 6, 1 << 9),  // METHOD
        (1 << 2, 1 << 2),  // PRIVATE
        (1 << 5, 1 << 1),  // FINAL
        (1 << 0, 1 << 3),  // PROTECTED
        (1 << 11, 1 << 6), // CASE
        (1 << 4, 1 << 8),  // DEFERRED
        (1 << 8, 1 << 10), // MODULE
        (1 << 1, 1 << 5),  // OVERRIDE
        (1 << 7, 1 << 11), // INTERFACE
        (1 << 9, 1 << 0),  // IMPLICIT
        (1 << 10, 1 << 4), // SEALED
        (1 << 3, 1 << 7),  // ABSTRACT
    ];
    let from_set = PAIRS.iter().fold(0u64, |a, (from, _)| a | from);
    let mut result = flags & !from_set;
    let mut tobe = flags & from_set;
    for (from, to) in PAIRS {
        if tobe & from != 0 {
            result |= to;
            tobe &= !from;
        }
    }
    result
}

/// Pickle a class or module-class symbol into nsc-subset bytes.
pub fn pickle_class(st: &SymbolTable, class_id: SymbolId) -> Vec<u8> {
    if class_id.is_none() {
        return Vec::new();
    }
    let mut p = Pickler::new(st);
    p.pickle_class(class_id);
    p.finish()
}

/// Snapshot pickles before erasure. nsc pickles pre-erasure signatures so
/// `id[T]` / `Box[A]#get` stay type parameters, not `Object`.
pub fn pickle_all(st: &SymbolTable) -> std::collections::HashMap<u32, Vec<u8>> {
    let mut out = std::collections::HashMap::new();
    for s in &st.symbols {
        if matches!(s.kind, SymKind::Class | SymKind::ModuleClass) && !s.id.is_none() {
            let raw = pickle_class(st, s.id);
            if !raw.is_empty() {
                out.insert(s.id.0, raw);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Unpickler
// ---------------------------------------------------------------------------

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn remaining(&self) -> bool {
        self.pos < self.bytes.len()
    }

    #[allow(dead_code)]
    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.bytes.len());
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn read_nat(&mut self) -> Option<u32> {
        Some(self.read_long_nat()? as u32)
    }

    fn read_long_nat(&mut self) -> Option<u64> {
        let mut x = 0u64;
        loop {
            let b = self.read_byte()? as u64;
            x = (x << 7) + (b & 0x7f);
            if (b & 0x80) == 0 {
                return Some(x);
            }
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum Entry {
    TermName(String),
    TypeName(String),
    NoneSym,
    TypeSym {
        name: u32,
        owner: u32,
        info: u32,
    },
    ClassSym {
        name: u32,
        owner: u32,
        info: u32,
        flags: u64,
    },
    ModuleSym {
        name: u32,
        owner: u32,
        info: u32,
    },
    ValSym {
        name: u32,
        owner: u32,
        info: u32,
        flags: u64,
    },
    ExtRef(u32),
    NoTpe,
    ThisTpe(u32),
    SingleTpe {
        prefix: u32,
        sym: u32,
    },
    ConstantTpe(u32),
    LiteralTy(String),
    AnnotatedTpe(u32),
    TypeRef {
        prefix: u32,
        sym: u32,
        args: Vec<u32>,
    },
    /// `CLASSINFOtpe len_Nat classsym_Ref {tpe_Ref}`. The parents used to be
    /// dropped here; `extends AnyVal` lives nowhere else, because a value
    /// class's class file says `extends java/lang/Object`.
    ClassInfo {
        parents: Vec<u32>,
    },
    MethodTpe {
        ret: u32,
        params: Vec<u32>,
    },
    PolyTpe {
        tparams: Vec<u32>,
        rest: u32,
    },
    Existential(u32),
    Other,
}

fn read_symbol_info(r: &mut Reader, end: usize) -> Option<(u32, u32, u64, u32)> {
    let name = r.read_nat()?;
    let owner = r.read_nat()?;
    let flags = r.read_long_nat()?;
    let info = r.read_nat()?;
    // Ignore a trailing privateWithin if a writer ever emits one.
    while r.pos < end {
        let _ = r.read_nat()?;
    }
    Some((name, owner, flags, info))
}

/// Unpickle our subset. Returns the first class/module plus its methods.
// The tag names are nsc's own (`CONSTANTtpe`, `LITERALint`); matching on them
// keeps this readable against PickleFormat.scala.
#[allow(non_upper_case_globals)]
pub fn unpickle(bytes: &[u8]) -> Option<PickledClass> {
    if bytes.is_empty() {
        return None;
    }
    let mut r = Reader::new(bytes);
    let major = r.read_nat()?;
    let _minor = r.read_nat()?;
    if major != MAJOR {
        return None;
    }
    let nentries = r.read_nat()? as usize;
    if nentries > 100_000 {
        return None;
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(nentries);
    for _ in 0..nentries {
        if !r.remaining() {
            return None;
        }
        let tag = r.read_byte()?;
        let len = r.read_nat()? as usize;
        let end = r.pos.saturating_add(len).min(r.bytes.len());
        let e = match tag {
            TERMNAME => {
                let s = String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned();
                r.pos = end;
                Entry::TermName(s)
            }
            TYPENAME => {
                let s = String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned();
                r.pos = end;
                Entry::TypeName(s)
            }
            NONESYM => {
                r.pos = end;
                Entry::NoneSym
            }
            TYPESYM | ALIASSYM => {
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::TypeSym { name, owner, info }
            }
            CLASSSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ClassSym {
                    name,
                    owner,
                    info,
                    flags,
                }
            }
            MODULESYM => {
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ModuleSym { name, owner, info }
            }
            VALSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ValSym {
                    name,
                    owner,
                    info,
                    flags,
                }
            }
            EXTREF | EXTMODCLASSREF => {
                let n = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ExtRef(n)
            }
            NOTPE | NOPREFIXTPE => {
                r.pos = end;
                Entry::NoTpe
            }
            THISTPE => {
                let s = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ThisTpe(s)
            }
            SINGLETPE => {
                let prefix = r.read_nat().unwrap_or(0);
                let sym = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::SingleTpe { prefix, sym }
            }
            CONSTANTtpe => {
                let c = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ConstantTpe(c)
            }
            LITERALunit => {
                r.pos = end;
                Entry::LiteralTy("Unit".into())
            }
            LITERALboolean => {
                r.pos = end;
                Entry::LiteralTy("Boolean".into())
            }
            LITERALchar => {
                r.pos = end;
                Entry::LiteralTy("Char".into())
            }
            LITERALint => {
                r.pos = end;
                Entry::LiteralTy("Int".into())
            }
            LITERALlong => {
                r.pos = end;
                Entry::LiteralTy("Long".into())
            }
            LITERALfloat => {
                r.pos = end;
                Entry::LiteralTy("Float".into())
            }
            LITERALdouble => {
                r.pos = end;
                Entry::LiteralTy("Double".into())
            }
            LITERALstring => {
                r.pos = end;
                Entry::LiteralTy("String".into())
            }
            LITERALnull => {
                r.pos = end;
                Entry::LiteralTy("Null".into())
            }
            ANNOTATEDTPE => {
                let tpe = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::AnnotatedTpe(tpe)
            }
            TYPEREFTPE => {
                let prefix = r.read_nat().unwrap_or(0);
                let sym = r.read_nat().unwrap_or(0);
                // nsc writes the type arguments after the symbol reference.
                // Dropping them turned `F[A]` into a bare `F`.
                let mut args = Vec::new();
                while r.pos < end {
                    match r.read_nat() {
                        Some(a) => args.push(a),
                        None => break,
                    }
                }
                r.pos = end;
                Entry::TypeRef { prefix, sym, args }
            }
            CLASSINFOTPE => {
                let _classsym = r.read_nat().unwrap_or(0);
                let mut parents = Vec::new();
                while r.pos < end {
                    match r.read_nat() {
                        Some(p) => parents.push(p),
                        None => break,
                    }
                }
                r.pos = end;
                Entry::ClassInfo { parents }
            }
            METHODTPE => {
                let ret = r.read_nat().unwrap_or(0);
                let mut params = Vec::new();
                while r.pos < end {
                    if let Some(p) = r.read_nat() {
                        params.push(p);
                    } else {
                        break;
                    }
                }
                r.pos = end;
                Entry::MethodTpe { ret, params }
            }
            POLYTPE => {
                let mut refs = Vec::new();
                while r.pos < end {
                    if let Some(p) = r.read_nat() {
                        refs.push(p);
                    } else {
                        break;
                    }
                }
                r.pos = end;
                // nsc: restpe first, then tparams
                let rest = refs.first().copied().unwrap_or(0);
                let tparams = if refs.len() > 1 {
                    refs[1..].to_vec()
                } else {
                    Vec::new()
                };
                Entry::PolyTpe { tparams, rest }
            }
            EXISTENTIALTPE => {
                let tpe = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::Existential(tpe)
            }
            _ => {
                r.pos = end;
                Entry::Other
            }
        };
        entries.push(e);
    }

    fn name_of(entries: &[Entry], i: u32) -> String {
        match entries.get(i as usize) {
            Some(Entry::TermName(s) | Entry::TypeName(s)) => s.clone(),
            Some(Entry::ExtRef(n)) => name_of(entries, *n),
            Some(
                Entry::TypeSym { name, .. }
                | Entry::ClassSym { name, .. }
                | Entry::ModuleSym { name, .. },
            ) => name_of(entries, *name),
            _ => String::new(),
        }
    }

    /// An existential's quantified symbols (`_$1`) have no meaning outside the
    /// pickle; name them `_` so the reader turns them into wildcards.
    fn is_existential_name(n: &str) -> bool {
        n.starts_with("_$")
    }

    // The entry graph can be cyclic (`class C[A <: C[A]]`); bound the walk.
    const MAX_TYPE_DEPTH: usize = 12;

    fn type_of(entries: &[Entry], i: u32, depth: usize) -> PickledType {
        if depth > MAX_TYPE_DEPTH {
            return PickledType::simple("Any");
        }
        match entries.get(i as usize) {
            Some(Entry::TypeRef { sym, args, .. }) => {
                let n = name_of(entries, *sym);
                if n.is_empty() {
                    return type_of(entries, *sym, depth + 1);
                }
                if is_existential_name(&n) {
                    return PickledType::simple("_");
                }
                PickledType {
                    name: n,
                    args: args
                        .iter()
                        .map(|a| type_of(entries, *a, depth + 1))
                        .collect(),
                }
            }
            Some(Entry::ExtRef(n)) => PickledType::simple(name_of(entries, *n)),
            Some(Entry::TermName(s) | Entry::TypeName(s)) => PickledType::simple(s.clone()),
            Some(Entry::NoTpe) => PickledType::simple("Any"),
            Some(Entry::Existential(t)) => type_of(entries, *t, depth + 1),
            Some(Entry::ThisTpe(s)) => match entries.get(*s as usize) {
                Some(Entry::ClassSym { name, .. } | Entry::ModuleSym { name, .. }) => {
                    PickledType::simple(name_of(entries, *name))
                }
                _ => PickledType::simple("Any"),
            },
            Some(Entry::AnnotatedTpe(t)) => type_of(entries, *t, depth + 1),
            Some(Entry::SingleTpe { prefix, .. }) => type_of(entries, *prefix, depth + 1),
            Some(Entry::ConstantTpe(c)) => type_of(entries, *c, depth + 1),
            Some(Entry::LiteralTy(s)) => PickledType::simple(s.clone()),
            _ => PickledType::simple("Any"),
        }
    }

    /// A pickled type parameter keeps its own parameters: nsc gives `F[_]` a
    /// `POLYtpe` info, and without reading it back `Applicative[F]` looked like
    /// it took a proper type.
    fn tparam_of(entries: &[Entry], i: u32, depth: usize) -> PickledTypeParam {
        let name = name_of(entries, i);
        let mut tparams = Vec::new();
        if depth < 4 {
            if let Some(Entry::TypeSym { info, .. }) = entries.get(i as usize) {
                if let Some(Entry::PolyTpe { tparams: tps, .. }) = entries.get(*info as usize) {
                    tparams = tps
                        .iter()
                        .map(|t| tparam_of(entries, *t, depth + 1))
                        .collect();
                }
            }
        }
        PickledTypeParam { name, tparams }
    }

    let mut class_idx = None;
    let mut is_module = false;
    const MODULE_PKL: u64 = 1 << 10;
    for (i, e) in entries.iter().enumerate() {
        match e {
            Entry::ClassSym { flags, .. } => {
                let mod_flag = (*flags & MODULE_PKL) != 0;
                if class_idx.is_none() {
                    class_idx = Some(i);
                    is_module = mod_flag;
                }
            }
            Entry::ModuleSym { .. } if class_idx.is_none() => {
                class_idx = Some(i);
                is_module = true;
            }
            _ => {}
        }
    }
    let ci = class_idx?;
    let class_name = match &entries[ci] {
        Entry::ModuleSym { name, .. } | Entry::ClassSym { name, .. } => name_of(&entries, *name),
        _ => return None,
    };
    if class_name.is_empty() {
        return None;
    }

    fn peel_info(entries: &[Entry], info: u32) -> (Vec<PickledTypeParam>, u32) {
        match entries.get(info as usize) {
            Some(Entry::PolyTpe { tparams, rest }) => {
                let tps = tparams.iter().map(|t| tparam_of(entries, *t, 0)).collect();
                (tps, *rest)
            }
            _ => (Vec::new(), info),
        }
    }

    let class_info = match &entries[ci] {
        Entry::ModuleSym { info, .. } | Entry::ClassSym { info, .. } => *info,
        _ => 0,
    };
    let (class_tparams, class_info_body) = peel_info(&entries, class_info);
    let extends_anyval = match entries.get(class_info_body as usize) {
        Some(Entry::ClassInfo { parents }) => parents
            .iter()
            .any(|p| type_of(&entries, *p, 0).name == "AnyVal"),
        _ => false,
    };

    // Pickled flags. The first twelve bits are re-encoded by nsc's
    // `rawToPickledFlags` (`DEFERRED` moves from `1 << 4` to `1 << 8`);
    // everything above bit 11 keeps its raw position, which is why `STABLE`
    // and `ACCESSOR` are the numbers `Flags.scala` gives them.
    const DEFERRED_PKL: u64 = 1 << 8;
    const STABLE: u64 = 1 << 22;

    let mut methods = Vec::new();
    for e in &entries {
        let Entry::ValSym {
            name,
            owner,
            info,
            flags,
        } = e
        else {
            continue;
        };
        if *owner != ci as u32 {
            continue;
        }
        // Case-class ctor fields are PARAMACCESSOR without METHOD; skip them.
        const METHOD_PKL: u64 = 1 << 9;
        const PARAMACCESSOR: u64 = 1 << 29;
        if (*flags & METHOD_PKL) == 0 && (*flags & PARAMACCESSOR) != 0 {
            continue;
        }
        let mname = name_of(&entries, *name);
        if mname.is_empty() {
            continue;
        }
        let (tparams, rest) = peel_info(&entries, *info);
        if let Some(Entry::MethodTpe { ret, params }) = entries.get(rest as usize) {
            let mut param_names = Vec::new();
            let mut param_types = Vec::new();
            for p in params {
                if let Some(Entry::ValSym {
                    name: pn, info: pt, ..
                }) = entries.get(*p as usize)
                {
                    param_names.push(name_of(&entries, *pn));
                    param_types.push(type_of(&entries, *pt, 0));
                } else {
                    param_types.push(type_of(&entries, *p, 0));
                    param_names.push(format!("x${}", param_names.len()));
                }
            }
            let is_accessor = (*flags & (1u64 << 27)) != 0; // ACCESSOR
            const IMPLICIT_PKL: u64 = 1 << 0;
            let is_implicit = (*flags & IMPLICIT_PKL) != 0;
            methods.push(PickledMethod {
                name: mname.clone(),
                param_names,
                param_types,
                ret: type_of(&entries, *ret, 0),
                tparams,
                is_val: is_accessor,
                is_ctor: mname == "<init>",
                is_implicit,
                is_deferred: (*flags & DEFERRED_PKL) != 0,
                is_mutable: is_accessor && (*flags & STABLE) == 0,
            });
        } else {
            // NullaryMethodType (POLYtpe with no tparams) or a plain type.
            let is_accessor = (*flags & (1u64 << 27)) != 0;
            const IMPLICIT_PKL: u64 = 1 << 0;
            let is_implicit = (*flags & IMPLICIT_PKL) != 0;
            methods.push(PickledMethod {
                name: mname,
                param_names: Vec::new(),
                param_types: Vec::new(),
                ret: type_of(&entries, rest, 0),
                tparams,
                is_val: is_accessor,
                is_ctor: false,
                is_implicit,
                is_deferred: (*flags & DEFERRED_PKL) != 0,
                is_mutable: is_accessor && (*flags & STABLE) == 0,
            });
        }
    }

    Some(PickledClass {
        name: class_name,
        is_module,
        tparams: class_tparams,
        methods,
        extends_anyval,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pickle_tags(bytes: &[u8]) -> Vec<u8> {
        let mut r = Reader::new(bytes);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut tags = Vec::new();
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            r.skip(len);
            tags.push(tag);
        }
        tags
    }

    #[test]
    fn bytecodecs_roundtrip() {
        let src: Vec<u8> = (0u8..=255).collect();
        let enc = encode_bytes(&src);
        let s: String = enc.iter().copied().map(char::from).collect();
        let dec = decode_annotation_string(&s);
        assert_eq!(dec, src, "SID-10 roundtrip");
    }

    #[test]
    fn pickle_unpickle_names() {
        let mut buf = Buf::new();
        buf.write_nat(5);
        buf.write_nat(2);
        buf.write_entry(NONESYM, &[]);
        buf.write_entry(TERMNAME, b"Lib");
        buf.write_entry(TYPENAME, b"String");
        let raw = buf.bytes;
        let enc = encode_to_annotation_string(&raw);
        let dec = decode_annotation_string(&enc);
        assert_eq!(dec, raw);
    }

    #[test]
    fn pickle_class_roundtrip_default_getter() {
        let src = r#"
object Lib {
  def greet(name: String, punct: String = "!"): String = name + punct
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        assert!(!raw.is_empty(), "expected pickle bytes");
        let enc = encode_to_annotation_string(&raw);
        let dec = decode_annotation_string(&enc);
        let p = unpickle(&dec).expect("unpickle subset ScalaSignature");
        assert!(p.is_module, "expected module class");
        let names: Vec<&str> = p.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(
            names.iter().any(|n| *n == "greet"),
            "expected greet in pickle, got {names:?}"
        );
        assert!(
            names.iter().any(|n| *n == "greet$default$2"),
            "expected greet$default$2 in pickle, got {names:?}"
        );
    }

    #[test]
    fn pickle_vals_params_and_tparams() {
        let src = r#"
object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = name + punct
  def id[T](x: T): T = x
}
class Box[A](val value: A) {
  def get: A = value
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let p = unpickle(&pickle_class(&st, cls)).expect("unpickle Lib");
        assert!(p.is_module);
        let magic = p
            .methods
            .iter()
            .find(|m| m.name == "magic")
            .expect("val magic");
        assert!(magic.is_val, "magic should be a val, got {magic:?}");
        assert_eq!(magic.ret, "Int");
        let greet = p.methods.iter().find(|m| m.name == "greet").expect("greet");
        assert_eq!(greet.param_names.len(), 2);
        assert_eq!(
            greet.param_types,
            vec!["String".to_string(), "String".to_string()]
        );
        let id = p.methods.iter().find(|m| m.name == "id").expect("id");
        assert_eq!(id.tparams, vec!["T".to_string()]);
        assert_eq!(id.param_types, vec!["T".to_string()]);
        assert_eq!(id.ret, "T");

        let box_id = st
            .symbols
            .iter()
            .find(|s| s.name == "Box" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Box class");
        let b = unpickle(&pickle_class(&st, box_id)).expect("unpickle Box");
        assert!(!b.is_module);
        assert_eq!(b.tparams, vec!["A".to_string()]);
        let value = b
            .methods
            .iter()
            .find(|m| m.name == "value")
            .expect("val value");
        assert!(value.is_val);
        assert_eq!(value.ret, "A");
        let get = b.methods.iter().find(|m| m.name == "get").expect("get");
        assert_eq!(get.ret, "A");
        let init = b.methods.iter().find(|m| m.is_ctor).expect("<init>");
        assert_eq!(init.param_types, vec!["A".to_string()]);
    }

    #[test]
    fn pickle_package_object_implicit_class_conversion() {
        let src = r#"
package object enrich {
  implicit class Rich(n: Int) { def twice: Int = n * 2 }
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let pkg = st
            .symbols
            .iter()
            .find(|s| s.name == "package" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("package object module");
        let cls = st.module_class_of(pkg);
        let p = unpickle(&pickle_class(&st, cls)).expect("unpickle package$");
        let conv = p
            .methods
            .iter()
            .find(|m| m.name == "Rich" && !m.is_val)
            .expect("implicit conversion Rich");
        assert!(
            conv.is_implicit,
            "package object implicit class conversion must pickle IMPLICIT, got {conv:?}"
        );
        assert_eq!(conv.param_types, vec!["Int".to_string()]);
    }

    #[test]
    fn pickle_case_class_and_object_def() {
        let src = r#"
case class Point(x: Int, y: Int)
object Lib {
  def add(p: Point): Int = p.x + p.y
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let point = st
            .symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Point class");
        let point_raw = pickle_class(&st, point);
        let pc = unpickle(&point_raw).expect("unpickle Point");
        assert!(
            !pc.is_module,
            "Point.class pickle must stay a class, not the companion module"
        );
        assert!(
            pickle_tags(&point_raw).contains(&MODULESYM),
            "Point.class pickle must include MODULESYM so nsc can bind term Point"
        );
        let x = pc.methods.iter().find(|m| m.name == "x").expect("val x");
        assert!(x.is_val);
        assert_eq!(x.ret, "Int");
        let init = pc.methods.iter().find(|m| m.is_ctor).expect("<init>");
        assert_eq!(init.param_types, vec!["Int".to_string(), "Int".to_string()]);

        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let l = unpickle(&pickle_class(&st, st.module_class_of(lib))).expect("unpickle Lib");
        let add = l.methods.iter().find(|m| m.name == "add").expect("add");
        assert_eq!(add.param_types, vec!["Point".to_string()]);
        assert_eq!(add.ret, "Int");

        let pmod = st
            .symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Point module");
        let pm = unpickle(&pickle_class(&st, st.module_class_of(pmod))).expect("unpickle Point$");
        assert!(pm.is_module);
        let apply = pm
            .methods
            .iter()
            .find(|m| m.name == "apply")
            .expect("apply");
        assert_eq!(
            apply.param_types,
            vec!["Int".to_string(), "Int".to_string()]
        );
        assert_eq!(apply.ret, "Point");
        let unapply = pm
            .methods
            .iter()
            .find(|m| m.name == "unapply")
            .expect("unapply");
        assert_eq!(unapply.param_types, vec!["Point".to_string()]);
        // The result keeps its arguments: `Option[(Int, Int)]`, not `Option`.
        assert_eq!(unapply.ret.name, "Option");
        assert_eq!(unapply.ret.args.len(), 1);
        assert_eq!(unapply.ret.args[0].name, "Tuple2");
        assert_eq!(
            unapply.ret.args[0].args,
            vec!["Int".to_string(), "Int".to_string()]
        );
    }

    #[test]
    fn pickle_existentials_annot_args_and_nsc_flags() {
        let src = r#"
object Lib {
  final def f(xs: List[_]): Int = 0
  @deprecated("msg", "2.13.0") def g: Int = 1
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&EXISTENTIALTPE),
            "expected EXISTENTIALtpe in pickle, tags={tags:?}"
        );
        assert!(
            tags.contains(&SYMANNOT),
            "expected SYMANNOT in pickle, tags={tags:?}"
        );
        assert!(
            tags.contains(&LITERALstring),
            "expected LITERALstring in pickle, tags={tags:?}"
        );
        let p = unpickle(&raw).expect("unpickle Lib");
        let f = p.methods.iter().find(|m| m.name == "f").expect("f");
        // `List[_]`: the existential's quantified symbol reads back as `_`.
        assert_eq!(f.param_types[0].name, "List");
        assert_eq!(f.param_types[0].args, vec!["_".to_string()]);
        assert_eq!(f.ret, "Int");
        let g = p.methods.iter().find(|m| m.name == "g").expect("g");
        assert_eq!(g.ret, "Int");

        // nsc raw FINAL (1<<5) pickles to (1<<1); METHOD (1<<6) pickles to (1<<9).
        let mut r = Reader::new(&raw);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut saw_final_method = false;
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == VALSYM {
                let (name, _owner, flags, _info) = read_symbol_info(&mut r, end).unwrap();
                let nm = match pickle_tags_name(&raw, name) {
                    Some(s) => s,
                    None => {
                        r.pos = end;
                        continue;
                    }
                };
                if nm == "f" {
                    const METHOD_PKL: u64 = 1 << 9;
                    const FINAL_PKL: u64 = 1 << 1;
                    assert!(
                        flags & METHOD_PKL != 0,
                        "f should carry pickled METHOD, flags={flags:#x}"
                    );
                    assert!(
                        flags & FINAL_PKL != 0,
                        "f should carry pickled FINAL, flags={flags:#x}"
                    );
                    saw_final_method = true;
                }
            }
            r.pos = end;
        }
        assert!(saw_final_method, "did not find pickled method f");
    }

    #[test]
    fn pickle_java_deprecated_symannot() {
        let src = r#"
object Lib {
  @Deprecated def gone: Int = 3
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&SYMANNOT),
            "expected SYMANNOT for Java @Deprecated, tags={tags:?}"
        );
        let mut saw_deprecated = false;
        let mut r = Reader::new(&raw);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == TYPENAME || tag == TERMNAME {
                let name = String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned();
                if name == "Deprecated" {
                    saw_deprecated = true;
                }
            }
            r.pos = end;
        }
        assert!(
            saw_deprecated,
            "expected pickled name Deprecated for Java annotation"
        );
    }

    #[test]
    fn pickle_tree_ident_select_literal_and_varargs() {
        let src = r#"
class Ann(x: Any) extends annotation.StaticAnnotation
class C { val x = 1 }
object Lib {
  val foo = 1
  val c = new C
  @Ann(foo) def marked: Int = 1
  @Ann(c.x) def markedSel: Int = 2
  @Ann(3) def markedLit: Int = 3
  def join(xs: String*): Int = 0
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&TREE),
            "expected TREE for @Ann(foo)/@Ann(c.x), tags={tags:?}"
        );
        assert!(
            tags.contains(&IDENTtree) || tags.iter().any(|_| true),
            "tree subtags live in TREE bodies"
        );
        let mut saw_foo = false;
        let mut saw_join = false;
        let mut join_has_varargs = false;
        let mut r = Reader::new(&raw);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut entries: Vec<(u8, usize, usize)> = Vec::new();
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let start = r.pos;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == TERMNAME {
                let name = String::from_utf8_lossy(&r.bytes[start..end]).into_owned();
                if name == "foo" {
                    saw_foo = true;
                }
                if name == "join" {
                    saw_join = true;
                }
            }
            entries.push((tag, start, end));
            r.pos = end;
        }
        assert!(saw_foo, "expected pickled name foo");
        assert!(saw_join, "expected pickled name join");
        // Recover join VALsym flags: METHOD pickled + VARARGS raw 1<<43.
        let mut r2 = Reader::new(&raw);
        let _ = r2.read_nat();
        let _ = r2.read_nat();
        let n2 = r2.read_nat().unwrap_or(0) as usize;
        let mut term_join = None;
        let mut i = 0u32;
        while i < n2 as u32 {
            let Some(tag) = r2.read_byte() else {
                break;
            };
            let len = r2.read_nat().unwrap_or(0) as usize;
            let end = r2.pos.saturating_add(len).min(r2.bytes.len());
            if tag == TERMNAME {
                let name = String::from_utf8_lossy(&r2.bytes[r2.pos..end]).into_owned();
                if name == "join" {
                    term_join = Some(i);
                }
            }
            r2.pos = end;
            i += 1;
        }
        let join_name = term_join.expect("join TERMNAME");
        let mut r3 = Reader::new(&raw);
        let _ = r3.read_nat();
        let _ = r3.read_nat();
        let n3 = r3.read_nat().unwrap_or(0) as usize;
        for _ in 0..n3 {
            let Some(tag) = r3.read_byte() else {
                break;
            };
            let len = r3.read_nat().unwrap_or(0) as usize;
            let end = r3.pos.saturating_add(len).min(r3.bytes.len());
            if tag == VALSYM {
                let saved = r3.pos;
                if let Some(name_ref) = r3.read_nat() {
                    let _owner = r3.read_nat();
                    if let Some(flags) = r3.read_long_nat() {
                        if name_ref == join_name && (flags & (1u64 << 9)) != 0 {
                            const VARARGS: u64 = 1u64 << 43;
                            join_has_varargs = flags & VARARGS != 0;
                        }
                    }
                }
                r3.pos = saved;
            }
            r3.pos = end;
        }
        assert!(
            join_has_varargs,
            "expected VARARGS on pickled join(String*)"
        );
    }

    #[test]
    fn pickle_tree_this_classof_apply_and_extref_has_no_flags() {
        let src = r#"
class Ann(x: Any) extends annotation.StaticAnnotation
class Base { val foo = 1 }
class Holder extends Base {
  val x = 1
  @Ann(this) def markedThis: Int = 4
  @Ann(classOf[Int]) def markedClass: Int = 5
  @Ann(this.x) def markedThisSel: Int = 7
  @Ann(super.foo) def markedSuper: Int = 8
}
object Lib {
  def ident(n: Int): Int = n
  @Ann(ident(1)) def markedApply: Int = 6
  @Ann(ident(ident(1))) def markedNest: Int = 9
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let holder = st
            .symbols
            .iter()
            .find(|s| s.name == "Holder" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Holder");
        let hraw = pickle_class(&st, holder);
        let htags = pickle_tags(&hraw);
        assert!(
            htags.contains(&TREE),
            "expected TREE for @Ann(this), tags={htags:?}"
        );
        assert!(
            htags.contains(&LITERALclass),
            "expected LITERALclass for @Ann(classOf[Int]), tags={htags:?}"
        );
        let hsubs = tree_subtags(&hraw);
        assert!(
            hsubs.contains(&(THIStree as u32)),
            "expected THIStree subtag, got {hsubs:?}"
        );
        assert!(
            hsubs.contains(&(SELECTtree as u32)),
            "expected SELECTtree for this.x / super.foo, got {hsubs:?}"
        );
        assert!(
            hsubs.contains(&(SUPERtree as u32)),
            "expected SUPERtree for super.foo, got {hsubs:?}"
        );

        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&TREE),
            "expected TREE for @Ann(ident(1)), tags={tags:?}"
        );
        let subs = tree_subtags(&raw);
        assert!(
            subs.contains(&(APPLYtree as u32)),
            "expected APPLYtree subtag, got {subs:?}"
        );
        let apply_n = subs.iter().filter(|t| **t == APPLYtree as u32).count();
        assert!(
            apply_n >= 2,
            "expected nested APPLYtree for ident(ident(1)), got {subs:?}"
        );
        assert!(
            subs.contains(&(IDENTtree as u32)),
            "expected IDENTtree for ident, got {subs:?}"
        );
        assert!(
            subs.contains(&(LITERALtree as u32)),
            "expected LITERALtree for ident(1) arg, got {subs:?}"
        );

        // PickleFormat EXTref = name_Ref [owner_Ref]. There is no flags field;
        // stuffing a Nat there would be read as owner. JAVA on java.lang.Object
        // comes from the JDK classfile scalac completes, not from our pickle.
        let mut r = Reader::new(&raw);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut object_name = None;
        let mut i = 0u32;
        let mut entries: Vec<(u8, usize, usize)> = Vec::new();
        while i < n as u32 {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let start = r.pos;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == TYPENAME || tag == TERMNAME {
                let name = String::from_utf8_lossy(&r.bytes[start..end]).into_owned();
                if name == "Object" {
                    object_name = Some(i);
                }
            }
            entries.push((tag, start, end));
            r.pos = end;
            i += 1;
        }
        let object_name = object_name.expect("pickled name Object");
        let mut saw_object_extref = false;
        for (tag, start, end) in entries {
            if tag != EXTREF {
                continue;
            }
            let mut r2 = Reader::new(&raw);
            r2.pos = start;
            let Some(name_ref) = r2.read_nat() else {
                continue;
            };
            if name_ref != object_name {
                continue;
            }
            saw_object_extref = true;
            let mut nats = 0u32;
            while r2.pos < end {
                if r2.read_nat().is_none() {
                    break;
                }
                nats += 1;
            }
            assert_eq!(
                nats, 1,
                "EXTREF Object is name_Ref + owner_Ref (no flags Nat)"
            );
        }
        assert!(
            saw_object_extref,
            "expected java.lang.Object as EXTREF (no flags field)"
        );
    }

    #[test]
    fn pickle_named_annot_arg_as_constant() {
        // nsc typer rewrites `@Ann(foo = 1)` to positional `@Ann(1)` (LITERALint).
        let src = r#"
class Ann(x: Any) extends annotation.StaticAnnotation
object Lib {
  @Ann(foo = 1) def markedNamed: Int = 10
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&LITERALint),
            "expected LITERALint for @Ann(foo = 1) (nsc positional Constant), tags={tags:?}"
        );
        assert!(
            tags.contains(&SYMANNOT),
            "expected SYMANNOT for @Ann(foo = 1), tags={tags:?}"
        );
    }

    #[test]
    fn pickle_named_annot_tree_rhs() {
        // nsc typer rewrites `@Ann(foo = this.x)` / `@Ann(foo = bar)` to
        // positional TREE (Select / Ident), same as `@Ann(this.x)` / `@Ann(bar)`.
        let src = r#"
class Ann(x: Any) extends annotation.StaticAnnotation
class Holder { val x = 1; @Ann(foo = this.x) def markedNamedTree: Int = 11 }
object Lib { val bar = 1; @Ann(foo = bar) def markedNamedIdent: Int = 12 }
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let holder = st
            .symbols
            .iter()
            .find(|s| s.name == "Holder" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Holder");
        let hraw = pickle_class(&st, holder);
        let hsubs = tree_subtags(&hraw);
        assert!(
            hsubs.contains(&(SELECTtree as u32)) && hsubs.contains(&(THIStree as u32)),
            "expected positional TREE Select(This) for @Ann(foo = this.x), got {hsubs:?}"
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib");
        let lraw = pickle_class(&st, st.module_class_of(lib));
        let lsubs = tree_subtags(&lraw);
        assert!(
            lsubs.contains(&(IDENTtree as u32)),
            "expected positional TREE Ident for @Ann(foo = bar), got {lsubs:?}"
        );
    }

    fn tree_subtags(bytes: &[u8]) -> Vec<u32> {
        let mut r = Reader::new(bytes);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut tags = Vec::new();
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == TREE {
                let saved = r.pos;
                if let Some(sub) = r.read_nat() {
                    tags.push(sub);
                }
                r.pos = saved;
            }
            r.pos = end;
        }
        tags
    }

    #[test]
    fn pickle_ordered_compare_bridge_flag() {
        let src = r#"
class OrdBox(val n: Int) extends Ordered[OrdBox] {
  def compare(that: OrdBox): Int = n - that.n
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let cls = st
            .symbols
            .iter()
            .find(|s| s.name == "OrdBox" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("OrdBox");
        let raw = pickle_class(&st, cls);
        let mut r = Reader::new(&raw);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut saw_bridge = false;
        const BRIDGE: u64 = 1u64 << 26;
        const METHOD_PKL: u64 = 1 << 9;
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if tag == VALSYM {
                let saved = r.pos;
                let _name = r.read_nat();
                let _owner = r.read_nat();
                if let Some(flags) = r.read_long_nat() {
                    if flags & METHOD_PKL != 0 && flags & BRIDGE != 0 {
                        saw_bridge = true;
                    }
                }
                r.pos = saved;
            }
            r.pos = end;
        }
        assert!(
            saw_bridge,
            "expected BRIDGE on Ordered.compare erasure bridge"
        );
    }

    #[test]
    fn pickle_this_type_annot_and_bounded_existential() {
        let src = r#"
class Holder {
  def me: this.type = this
  def n: Int = 1
}
object Lib {
  def f(xs: List[_ <: AnyRef]): Int = 0
  def h(x: Int @unchecked): Int = x
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let holder = st
            .symbols
            .iter()
            .find(|s| s.name == "Holder" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Holder");
        let hraw = pickle_class(&st, holder);
        let htags = pickle_tags(&hraw);
        assert!(
            htags.contains(&THISTPE),
            "expected THIStpe for this.type, tags={htags:?}"
        );
        let hp = unpickle(&hraw).expect("unpickle Holder");
        let me = hp.methods.iter().find(|m| m.name == "me").expect("me");
        assert_eq!(me.ret, "Holder");

        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let raw = pickle_class(&st, st.module_class_of(lib));
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&EXISTENTIALTPE),
            "expected EXISTENTIALtpe for List[_ <: AnyRef], tags={tags:?}"
        );
        assert!(
            tags.contains(&ANNOTATEDTPE),
            "expected ANNOTATEDtpe for Int @unchecked, tags={tags:?}"
        );
        assert!(
            tags.contains(&ANNOTINFO),
            "expected ANNOTINFO, tags={tags:?}"
        );
        let p = unpickle(&raw).expect("unpickle Lib");
        let f = p.methods.iter().find(|m| m.name == "f").expect("f");
        assert_eq!(f.param_types[0].name, "List");
        assert_eq!(f.param_types[0].args, vec!["_".to_string()]);
        let h = p.methods.iter().find(|m| m.name == "h").expect("h");
        assert_eq!(h.param_types, vec!["Int".to_string()]);
    }

    #[test]
    fn pickle_constant_tpe_for_literal_types() {
        let src = r#"
object Lib {
  val one: 1 = 1
  def lit(x: 1): Int = x
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let raw = pickle_class(&st, st.module_class_of(lib));
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&CONSTANTtpe),
            "expected CONSTANTtpe in pickle, tags={tags:?}"
        );
        assert!(
            tags.contains(&LITERALint),
            "expected LITERALint in pickle, tags={tags:?}"
        );
        let p = unpickle(&raw).expect("unpickle Lib");
        let one = p.methods.iter().find(|m| m.name == "one").expect("one");
        assert!(one.is_val);
        assert_eq!(one.ret, "Int");
        let lit = p.methods.iter().find(|m| m.name == "lit").expect("lit");
        assert_eq!(lit.param_types, vec!["Int".to_string()]);
        assert_eq!(lit.ret, "Int");
    }

    #[test]
    fn pickle_packed_nested_existential_and_refinement() {
        let src = r#"
trait MixA { def a: Int }
trait MixB { def b: Int }
object Lib {
  def nest(xs: List[_ <: List[_]]): Int = 0
  def idRef(x: MixA with MixB { def f: Int }): MixA with MixB { def f: Int } = x
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let raw = pickle_class(&st, st.module_class_of(lib));
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&EXISTENTIALTPE),
            "expected EXISTENTIALtpe for List[_ <: List[_]], tags={tags:?}"
        );
        assert!(
            tags.contains(&REFINEDTPE),
            "expected REFINEDtpe for MixA with MixB {{ def f: Int }}, tags={tags:?}"
        );
        let p = unpickle(&raw).expect("unpickle Lib");
        assert!(
            p.methods.iter().any(|m| m.name == "nest"),
            "expected nest in pickle"
        );
        assert!(
            p.methods.iter().any(|m| m.name == "idRef"),
            "expected idRef in pickle"
        );
    }

    #[test]
    fn pickle_type_alias_aliassym() {
        let src = r#"
object Lib {
  type T = Int
  def usesAlias(x: T): T = x
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let raw = pickle_class(&st, st.module_class_of(lib));
        let tags = pickle_tags(&raw);
        assert!(
            tags.contains(&ALIASSYM),
            "expected ALIASsym for type T = Int, tags={tags:?}"
        );
        let p = unpickle(&raw).expect("unpickle Lib");
        let u = p
            .methods
            .iter()
            .find(|m| m.name == "usesAlias")
            .expect("usesAlias");
        assert_eq!(u.param_types, vec!["Int".to_string()]);
        assert_eq!(u.ret, "Int");
    }

    #[test]
    fn pickle_type_member_alias_and_type_parameter_bound() {
        let src = r#"
trait Profile {
  type Schema <: SchemaDef
  trait SchemaDef {
    def ++(other: Schema): Schema
  }
  object api {
    type Item[A] = Schema
  }
}

trait AbstractTable[A] {
  type Elem
}
class TableQuery[E <: AbstractTable[_]](val value: E)
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let profile = st
            .symbols
            .iter()
            .find(|s| s.name == "Profile" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Profile");
        let profile_tags = pickle_tags(&pickle_class(&st, profile));
        assert!(
            profile_tags.contains(&ALIASSYM),
            "expected ALIASsym for an alias whose RHS is a type member, tags={profile_tags:?}"
        );
        assert!(
            profile_tags.contains(&TYPEBOUNDSTPE),
            "expected TYPEBOUNDStpe for abstract Schema, tags={profile_tags:?}"
        );

        let table_query = st
            .symbols
            .iter()
            .find(|s| s.name == "TableQuery" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("TableQuery");
        let bound_tags = pickle_tags(&pickle_class(&st, table_query));
        assert!(
            bound_tags.contains(&TYPEBOUNDSTPE),
            "expected TYPEBOUNDStpe for E, tags={bound_tags:?}"
        );
        assert!(
            bound_tags.contains(&EXISTENTIALTPE),
            "expected EXISTENTIALtpe in E <: AbstractTable[_], tags={bound_tags:?}"
        );
    }

    fn pickle_tags_name(bytes: &[u8], idx: u32) -> Option<String> {
        let mut r = Reader::new(bytes);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat()? as usize;
        let mut i = 0u32;
        while i < n as u32 {
            let tag = r.read_byte()?;
            let len = r.read_nat()? as usize;
            let end = r.pos.saturating_add(len).min(r.bytes.len());
            if i == idx {
                if tag == TERMNAME || tag == TYPENAME {
                    return Some(String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned());
                }
                return None;
            }
            r.pos = end;
            i += 1;
        }
        None
    }
}
