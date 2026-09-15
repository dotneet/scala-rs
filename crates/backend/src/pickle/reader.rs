//! Behavior-preserving reader for the nsc PickleFormat subset emitted by the writer.

use super::*;

pub(super) struct Reader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) pos: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    pub(super) fn remaining(&self) -> bool {
        self.pos < self.bytes.len()
    }

    #[allow(dead_code)]
    pub(super) fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.bytes.len());
    }

    pub(super) fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Some(b)
    }

    pub(super) fn read_nat(&mut self) -> Option<u32> {
        Some(self.read_long_nat()? as u32)
    }

    pub(super) fn read_long_nat(&mut self) -> Option<u64> {
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
        alias: bool,
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
    ExtRef {
        name: u32,
        owner: Option<u32>,
    },
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
    TypeBounds {
        lo: u32,
        hi: u32,
    },
    RefinedTpe {
        parents: Vec<u32>,
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

pub(super) fn read_symbol_info(
    r: &mut Reader,
    end: usize,
    entry_tags: &[u8],
) -> Option<(u32, u32, u64, u32)> {
    let name = r.read_nat()?;
    let owner = r.read_nat()?;
    let flags = r.read_long_nat()?;
    let first = r.read_nat()?;
    // `privateWithin` is an optional symbol reference before `info`. The
    // subset reader does not expose that field, but it must skip it to keep
    // reading our own qualified-private class/object pickles. An info entry
    // is a type entry, whereas a boundary is a class/module or external
    // symbol reference, so the entry tag disambiguates the two Nat values.
    let is_private_within = matches!(
        entry_tags.get(first as usize),
        Some(tag) if matches!(*tag, EXTREF | EXTMODCLASSREF | CLASSSYM | MODULESYM)
    );
    let info = if is_private_within {
        r.read_nat()?
    } else {
        first
    };
    // Ignore any future trailing fields.
    while r.pos < end {
        let _ = r.read_nat()?;
    }
    Some((name, owner, flags, info))
}

/// Unpickle our subset. Returns the first class/module plus its methods.
// The tag names are nsc's own (`CONSTANTtpe`, `LITERALint`); matching on them
// keeps this readable against PickleFormat.scala.
#[allow(non_upper_case_globals)]
pub(super) fn unpickle(bytes: &[u8]) -> Option<PickledClass> {
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
    // Build the tag table before parsing symbol bodies so an optional
    // `privateWithin` reference can be distinguished from the info ref even
    // when the referenced entry appears later in the pickle.
    let mut tag_reader = Reader::new(bytes);
    tag_reader.pos = r.pos;
    let mut entry_tags = Vec::with_capacity(nentries);
    for _ in 0..nentries {
        let tag = tag_reader.read_byte()?;
        let len = tag_reader.read_nat()? as usize;
        let end = tag_reader.pos.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        tag_reader.pos = end;
        entry_tags.push(tag);
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
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end, &entry_tags)?;
                r.pos = end;
                Entry::TypeSym {
                    name,
                    owner,
                    info,
                    alias: tag == ALIASSYM,
                }
            }
            CLASSSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end, &entry_tags)?;
                r.pos = end;
                Entry::ClassSym {
                    name,
                    owner,
                    info,
                    flags,
                }
            }
            MODULESYM => {
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end, &entry_tags)?;
                r.pos = end;
                Entry::ModuleSym { name, owner, info }
            }
            VALSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end, &entry_tags)?;
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
                let owner = if r.pos < end { r.read_nat() } else { None };
                r.pos = end;
                Entry::ExtRef { name: n, owner }
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
            TYPEBOUNDSTPE => {
                let lo = r.read_nat().unwrap_or(0);
                let hi = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::TypeBounds { lo, hi }
            }
            REFINEDTPE => {
                // REFINEDtpe = refinement-class ref followed by parent refs.
                // The synthetic class carries the declarations; the compact
                // classpath ABI only needs the parent intersection for bounds.
                let _refinement = r.read_nat().unwrap_or(0);
                let mut parents = Vec::new();
                while r.pos < end {
                    if let Some(parent) = r.read_nat() {
                        parents.push(parent);
                    } else {
                        break;
                    }
                }
                r.pos = end;
                Entry::RefinedTpe { parents }
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
            Some(Entry::ExtRef { name, .. }) => name_of(entries, *name),
            Some(
                Entry::TypeSym { name, .. }
                | Entry::ClassSym { name, .. }
                | Entry::ModuleSym { name, .. },
            ) => name_of(entries, *name),
            _ => String::new(),
        }
    }

    /// Resolve an external reference's owner chain into the dotted source
    /// name that classpath lookup needs. `name_of` intentionally keeps the
    /// simple leaf name for members; type refs to external classes need the
    /// package because a Scala pickle's `EXTref` carries that owner separately.
    fn external_name(entries: &[Entry], name: u32, owner: Option<u32>, depth: usize) -> String {
        if depth > MAX_TYPE_DEPTH {
            return name_of(entries, name);
        }
        let leaf = name_of(entries, name);
        let Some(owner) = owner else {
            return leaf;
        };
        let prefix = match entries.get(owner as usize) {
            Some(Entry::ExtRef {
                name: owner_name,
                owner: owner_owner,
            }) => external_name(entries, *owner_name, *owner_owner, depth + 1),
            _ => String::new(),
        };
        if prefix.is_empty() || prefix == "<empty>" || prefix == "<root>" {
            leaf
        } else {
            format!("{prefix}.{leaf}")
        }
    }

    /// Keep the compact reader's historical leaf spelling for standard
    /// library references. Their owners are still retained for resolving
    /// non-standard external classes (for example `slick.ast.BaseTypedType`),
    /// but exposing `scala.Int` or `scala.package.List` here would change the
    /// ABI returned by the reader and break existing callers.
    fn external_type_name(
        entries: &[Entry],
        name: u32,
        owner: Option<u32>,
        depth: usize,
    ) -> String {
        let qualified = external_name(entries, name, owner, depth);
        if qualified.starts_with("scala.")
            || qualified.starts_with("java.")
            || qualified.starts_with("javax.")
        {
            name_of(entries, name)
        } else {
            qualified
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
                let n = match entries.get(*sym as usize) {
                    Some(Entry::ExtRef { name, owner }) => {
                        external_type_name(entries, *name, *owner, depth + 1)
                    }
                    _ => name_of(entries, *sym),
                };
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
                    singleton: false,
                }
            }
            Some(Entry::ExtRef { name, owner }) => {
                PickledType::simple(external_type_name(entries, *name, *owner, depth + 1))
            }
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
            Some(Entry::SingleTpe { sym, .. }) => {
                let mut ty = type_of(entries, *sym, depth + 1);
                ty.singleton = true;
                ty
            }
            Some(Entry::ConstantTpe(c)) => type_of(entries, *c, depth + 1),
            Some(Entry::RefinedTpe { parents }) => PickledType::intersection(
                parents
                    .iter()
                    .map(|p| type_of(entries, *p, depth + 1))
                    .collect(),
            ),
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

    // `type` declarations have no JVM member to recover them from, so keep
    // their ScalaSignature entries in the classpath ABI as well. In
    // particular, `RelationalProfile.SchemaDescription` is inherited by the
    // API's `schemaActionExtensionMethods` parameter and must not become an
    // unresolved `Named` type when the profile is supplied as classfiles.
    let mut type_members = Vec::new();
    for e in &entries {
        let Entry::TypeSym {
            name,
            owner,
            info,
            alias,
        } = e
        else {
            continue;
        };
        if *owner != ci as u32 {
            continue;
        }
        let name = crate::classfile::decode_method_name(&name_of(&entries, *name));
        if name.is_empty() {
            continue;
        }
        let (tparams, rest) = peel_info(&entries, *info);
        if *alias {
            type_members.push(PickledTypeMember {
                name,
                lower_bound: PickledType::simple("Nothing"),
                upper_bound: PickledType::simple("Any"),
                alias: Some(type_of(&entries, rest, 0)),
                tparams,
            });
        } else if let Some(Entry::TypeBounds { lo, hi }) = entries.get(rest as usize) {
            type_members.push(PickledTypeMember {
                name,
                lower_bound: type_of(&entries, *lo, 0),
                upper_bound: type_of(&entries, *hi, 0),
                alias: None,
                tparams,
            });
        } else {
            // Keep malformed/older abstract entries usable with the same
            // conservative bounds the source typechecker uses for an
            // unbounded member rather than dropping the declaration.
            type_members.push(PickledTypeMember {
                name,
                lower_bound: PickledType::simple("Nothing"),
                upper_bound: PickledType::simple("Any"),
                alias: None,
                tparams,
            });
        }
    }

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
        // Macro declarations have no JVM method. Their full signature and
        // implementation binding are supplied by PickleSupply, never by this
        // flat eager method reader (which cannot preserve access or macroImpl).
        if *flags & (1 << 15) != 0 {
            continue;
        }
        // Case-class ctor fields are PARAMACCESSOR without METHOD; skip them.
        const METHOD_PKL: u64 = 1 << 9;
        const PARAMACCESSOR: u64 = 1 << 29;
        if (*flags & METHOD_PKL) == 0 && (*flags & PARAMACCESSOR) != 0 {
            continue;
        }
        let mname = crate::classfile::decode_method_name(&name_of(&entries, *name));
        if mname.is_empty() {
            continue;
        }
        let (tparams, rest) = peel_info(&entries, *info);
        if let Some(Entry::MethodTpe {
            ret: first_ret,
            params: first_params,
        }) = entries.get(rest as usize)
        {
            // A method is pickled as one method type per source parameter
            // clause. This subset reader models a member as one flat list --
            // the classpath ABI's JVM parameter order -- so every clause has
            // to be joined onto it. Keeping only the first left the *result*
            // pointing at the next clause's `METHODtpe`, which `type_of` has
            // no reading for and answers `Any` to: `def cur(a: Int)(b: Int)`
            // came back as `cur(a: Int): Any` and `cur(1)(2)` reported "value
            // apply is not a member of Any". It only ever showed on a
            // constructor before, because that was the one case with the
            // loop.
            let mut params = first_params.clone();
            let mut clause_sizes = vec![first_params.len()];
            let mut ret = *first_ret;
            while let Some(Entry::MethodTpe {
                ret: next_ret,
                params: next_params,
            }) = entries.get(ret as usize)
            {
                clause_sizes.push(next_params.len());
                params.extend(next_params.iter().copied());
                ret = *next_ret;
            }
            let mut param_names = Vec::new();
            let mut param_types = Vec::new();
            let mut param_flags = Vec::new();
            for p in &params {
                if let Some(Entry::ValSym {
                    name: pn,
                    info: pt,
                    flags: pf,
                    ..
                }) = entries.get(*p as usize)
                {
                    param_names.push(crate::classfile::decode_method_name(&name_of(
                        &entries, *pn,
                    )));
                    param_types.push(type_of(&entries, *pt, 0));
                    param_flags.push(*pf);
                } else {
                    param_types.push(type_of(&entries, *p, 0));
                    param_names.push(format!("x${}", param_names.len()));
                    param_flags.push(0);
                }
            }
            let is_accessor = (*flags & (1u64 << 27)) != 0; // ACCESSOR
            const IMPLICIT_PKL: u64 = 1 << 0;
            let is_implicit = (*flags & IMPLICIT_PKL) != 0;
            methods.push(PickledMethod {
                name: mname.clone(),
                param_names,
                param_types,
                clause_sizes,
                param_flags,
                ret: type_of(&entries, ret, 0),
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
                clause_sizes: Vec::new(),
                param_flags: Vec::new(),
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
        type_members,
        extends_anyval,
    })
}
