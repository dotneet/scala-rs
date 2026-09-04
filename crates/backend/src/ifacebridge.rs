//! Class-level bridges and forwarders for members inherited from *binary*
//! interfaces.
//!
//! nsc's mixin phase gives every class a forwarder for each concrete member it
//! inherits from a trait (`-Xmixin-force-forwarders`, on by default), plus a
//! bridge for every erased overload those forwarders introduce: an anonymous
//! `immutable.IndexedSeq` gets about 250 of them. scala-rs emits the
//! interface's default method and lets the JVM find it, which is right until
//! the JVM's own resolution disagrees with Scala's. It does so in exactly two
//! places, and this pass covers those two.
//!
//! **Erased overloads.** `scala.collection.IterableOps.iterableFactory` is
//! `()Lscala/collection/IterableFactory;`, `collection.Iterable` and
//! `immutable.Iterable` implement it at that descriptor, and `Seq` /
//! `immutable.Seq` / `IndexedSeq` / `immutable.IndexedSeq` override it
//! covariantly at `()Lscala/collection/SeqFactory;`. The library's *interfaces*
//! carry no bridge between the two — nsc puts it on the implementing class —
//! so for
//!
//! ```scala
//! new immutable.IndexedSeq[T] { def apply(i: Int) = …; def length = … }
//! ```
//!
//! a wide `iterableFactory()Lscala/collection/IterableFactory;` call resolves
//! to `immutable.Iterable`'s default: the maximally specific super-interface
//! *for that descriptor* (JVMS 5.4.3.3). `IterableFactoryDefaults.newSpecificBuilder`
//! makes exactly that call, so `groupBy` on such an anonymous class built
//! `List`s and slick's `ExpandTables` died with `$colon$colon cannot be cast to
//! IndexedSeq`; `filter` was an `AbstractMethodError` on `fromSpecific`, whose
//! only implementation is `IterableFactoryDefaults`' at the narrower
//! `()Lscala/collection/IterableOps;`.
//!
//! **`toString` / `hashCode` / `equals`.** A method inherited from the
//! superclass always beats an interface default (JVMS 5.4.3.3 again), and
//! every class has `java.lang.Object` above it, so `Seq`'s `toString` never
//! ran: `ConstArray.toSeq.toString` printed
//! `slick.util.ConstArray$$anon$630@281e3708` where scalac prints
//! `IndexedSeq(1, 2, 3, 4)`. nsc's forwarder is `invokestatic
//! scala/collection/Seq.toString$`, and only that shape works — the defining
//! interface is usually not a *direct* super-interface, which `invokespecial`
//! would require.
//!
//! The member set has to come from the parents' class files. The symbol table
//! only holds what the typer was asked for, and nothing in slick ever names
//! `iterableFactory`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use scala_rs_pickle::classfile::{parse_cp, skip_attrs, Cursor};
use scala_rs_typer::javaclass::BinaryIndex;

const ACC_STATIC: u16 = 0x0008;
const ACC_BRIDGE: u16 = 0x0040;
const ACC_INTERFACE: u16 = 0x0200;
const ACC_ABSTRACT: u16 = 0x0400;

/// The members `java.lang.Object` declares that a Scala trait may also
/// implement. Inherited from the superclass, they win over any interface
/// default, so the class needs a forwarder or the trait's version never runs.
const OBJECT_CLASH: [(&str, &str); 3] = [
    ("toString", "()Ljava/lang/String;"),
    ("hashCode", "()I"),
    ("equals", "(Ljava/lang/Object;)Z"),
];

/// How a bridge reaches the implementation it stands in front of.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BridgeKind {
    /// `invokevirtual this.<name>:<desc>` — the same method at its narrower
    /// erased descriptor, wherever the JVM ends up resolving that.
    Narrow(String),
    /// `invokestatic <interface>.<helper>:<desc>` — the trait's own
    /// implementation, named directly because no other spelling reaches it.
    Static {
        iface: String,
        helper: String,
        desc: String,
    },
}

/// One bridge to emit on the class under construction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bridge {
    /// Method name, in class-file spelling (`$plus$colon`, not `+:`).
    pub name: String,
    /// The descriptor the bridge is declared at.
    pub desc: String,
    pub kind: BridgeKind,
}

/// One declaration of a `(name, parameters)` signature: which super-type
/// declared it (an index into the closure), its erased return descriptor, and
/// whether it is abstract.
type Decl = (usize, String, bool);

/// What this pass needs from one class file.
struct Info {
    is_interface: bool,
    supers: Vec<String>,
    /// name, descriptor, access flags — synthetic and bridge members included,
    /// because a trait's `m$` static implementation is `ACC_SYNTHETIC`.
    methods: Vec<(String, String, u16)>,
}

/// Class name, super types and method table. Deliberately not
/// `scala_rs_typer::javaclass::parse_java_classfile`, which drops synthetic
/// and bridge members.
fn parse(bytes: &[u8]) -> Option<Info> {
    let mut c = Cursor::new(bytes);
    if c.u4()? != 0xCAFEBABE {
        return None;
    }
    let _minor = c.u2()?;
    let _major = c.u2()?;
    let cp = parse_cp(&mut c)?;
    let access = c.u2()?;
    let _this = c.u2()?;
    let super_i = c.u2()?;
    let mut supers = Vec::new();
    let niface = c.u2()? as usize;
    for _ in 0..niface {
        let i = c.u2()?;
        if let Some(n) = cp.class_name(i) {
            supers.push(n);
        }
    }
    if super_i != 0 {
        if let Some(n) = cp.class_name(super_i) {
            supers.push(n);
        }
    }
    let nfields = c.u2()? as usize;
    for _ in 0..nfields {
        let _ = c.u2()?;
        let _ = c.u2()?;
        let _ = c.u2()?;
        skip_attrs(&mut c)?;
    }
    let nmethods = c.u2()? as usize;
    let mut methods = Vec::new();
    for _ in 0..nmethods {
        let acc = c.u2()?;
        let name_i = c.u2()?;
        let desc_i = c.u2()?;
        let name = cp.utf8(name_i)?;
        let desc = cp.utf8(desc_i)?;
        skip_attrs(&mut c)?;
        if name != "<init>" && name != "<clinit>" {
            methods.push((name, desc, acc));
        }
    }
    Some(Info {
        is_interface: access & ACC_INTERFACE != 0,
        supers,
        methods,
    })
}

/// Lazily parsed super-type class files, shared by every unit of a run.
pub struct BinaryParents {
    idx: RefCell<BinaryIndex>,
    cache: RefCell<HashMap<String, Option<Rc<Info>>>>,
}

impl std::fmt::Debug for BinaryParents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BinaryParents")
    }
}

/// Split `(params)Lret;` into the parameter part (with parentheses) and the
/// return descriptor.
fn split_desc(desc: &str) -> Option<(&str, &str)> {
    let cut = desc.find(')')?;
    Some((&desc[..=cut], &desc[cut + 1..]))
}

/// The internal name of a `Lpkg/Cls;` descriptor; `None` for primitives and
/// arrays, which are never bridged here.
fn class_of(desc: &str) -> Option<&str> {
    desc.strip_prefix('L')?.strip_suffix(';')
}

impl BinaryParents {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        BinaryParents {
            idx: RefCell::new(BinaryIndex::from_user_paths(paths)),
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn info(&self, name: &str) -> Option<Rc<Info>> {
        if let Some(hit) = self.cache.borrow().get(name) {
            return hit.clone();
        }
        let parsed = self
            .idx
            .borrow_mut()
            .find_class(name)
            .ok()
            .flatten()
            .and_then(|bytes| parse(&bytes))
            .map(Rc::new);
        self.cache
            .borrow_mut()
            .insert(name.to_string(), parsed.clone());
        parsed
    }

    /// Is `a` a sub-type of `b`? Everything is a sub-type of `java/lang/Object`.
    fn is_sub(&self, a: &str, b: &str) -> bool {
        if a == b || b == "java/lang/Object" {
            return true;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue = vec![a.to_string()];
        while let Some(n) = queue.pop() {
            if !seen.insert(n.clone()) || seen.len() > 512 {
                continue;
            }
            let Some(i) = self.info(&n) else { continue };
            for s in &i.supers {
                if s == b {
                    return true;
                }
                queue.push(s.clone());
            }
        }
        false
    }

    /// Every super-type of `roots` whose class file is on the binary path,
    /// `roots` included. A parent compiled in this same run is not on the
    /// path: it drops out, and the walk continues through whatever ancestors
    /// of it the caller also passed in `roots`.
    fn closure(&self, roots: &[String]) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        let mut queue: Vec<String> = roots.to_vec();
        let mut i = 0;
        while i < queue.len() && out.len() <= 512 {
            let n = queue[i].clone();
            i += 1;
            if n == "java/lang/Object" || !seen.insert(n.clone()) {
                continue;
            }
            let Some(info) = self.info(&n) else { continue };
            out.push(n);
            for s in &info.supers {
                queue.push(s.clone());
            }
        }
        out
    }

    /// The bridges a class with these super-types needs, given the methods it
    /// already declares (name and descriptor, class-file spelling).
    pub fn bridges(&self, roots: &[String], have: &HashSet<(String, String)>) -> Vec<Bridge> {
        let owners = self.closure(roots);
        if owners.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<Bridge> = Vec::new();
        self.covariant_bridges(&owners, have, &mut out);
        self.object_clash_forwarders(&owners, have, &mut out);
        // Deterministic output: the class file's method order must not depend
        // on hash iteration order.
        out.sort();
        out
    }

    /// A member declared with two different erased return types along the
    /// super-type chain needs the wide spelling on the class, forwarding to
    /// the narrow one.
    fn covariant_bridges(
        &self,
        owners: &[String],
        have: &HashSet<(String, String)>,
        out: &mut Vec<Bridge>,
    ) {
        // (name, parameter descriptor) -> (owner index, return descriptor, abstract)
        let mut by_sig: HashMap<(String, String), Vec<Decl>> = HashMap::new();
        for (oi, owner) in owners.iter().enumerate() {
            let Some(info) = self.info(owner) else {
                continue;
            };
            for (name, desc, acc) in &info.methods {
                if acc & (ACC_STATIC | ACC_BRIDGE) != 0 {
                    continue;
                }
                let Some((params, ret)) = split_desc(desc) else {
                    continue;
                };
                by_sig
                    .entry((name.clone(), params.to_string()))
                    .or_default()
                    .push((oi, ret.to_string(), acc & ACC_ABSTRACT != 0));
            }
        }
        for ((name, params), cands) in by_sig {
            let rets: HashSet<&str> = cands.iter().map(|(_, r, _)| r.as_str()).collect();
            if rets.len() < 2 {
                continue;
            }
            // The most specific declaration: the one whose owner is a sub-type
            // of every other owner. Ambiguity (two unrelated interfaces) is
            // left alone — that is a linearization question, not an erased
            // overload.
            let mut target: Option<&str> = None;
            let mut ambiguous = false;
            for (oi, ret, _) in &cands {
                let most = cands
                    .iter()
                    .all(|(oj, _, _)| oi == oj || self.is_sub(&owners[*oi], &owners[*oj]));
                if !most {
                    continue;
                }
                match target {
                    None => target = Some(ret),
                    Some(t) if t == ret => {}
                    Some(_) => ambiguous = true,
                }
            }
            let (Some(target), false) = (target, ambiguous) else {
                continue;
            };
            let target_desc = format!("{params}{target}");
            // Forwarding to a declaration nothing implements would turn a
            // wrong answer into an `AbstractMethodError`.
            let reachable = cands.iter().any(|(_, r, abs)| r == target && !abs)
                || have.contains(&(name.clone(), target_desc.clone()));
            if !reachable {
                continue;
            }
            let Some(tc) = class_of(target) else { continue };
            for wide in rets {
                if wide == target {
                    continue;
                }
                let Some(wc) = class_of(wide) else { continue };
                // `areturn` needs the value to be assignable to the declared
                // return type; only bridge when the class files say it is.
                if !self.is_sub(tc, wc) {
                    continue;
                }
                let desc = format!("{params}{wide}");
                if have.contains(&(name.clone(), desc.clone())) {
                    continue;
                }
                out.push(Bridge {
                    name: name.clone(),
                    desc,
                    kind: BridgeKind::Narrow(target_desc.clone()),
                });
            }
        }
    }

    /// `toString` / `hashCode` / `equals` implemented by a trait: `Object`'s
    /// wins unless the class says otherwise.
    fn object_clash_forwarders(
        &self,
        owners: &[String],
        have: &HashSet<(String, String)>,
        out: &mut Vec<Bridge>,
    ) {
        for (name, desc) in OBJECT_CLASH {
            if have.contains(&(name.to_string(), desc.to_string())) {
                continue;
            }
            // A real superclass implementation is the one Scala means too.
            if owners.iter().any(|o| {
                self.info(o).is_some_and(|i| {
                    !i.is_interface
                        && i.methods.iter().any(|(n, d, a)| {
                            n == name && d == desc && a & (ACC_STATIC | ACC_ABSTRACT) == 0
                        })
                })
            }) {
                continue;
            }
            let mut best: Option<&str> = None;
            let mut ambiguous = false;
            for o in owners {
                let Some(i) = self.info(o) else { continue };
                if !i.is_interface {
                    continue;
                }
                if !i
                    .methods
                    .iter()
                    .any(|(n, d, a)| n == name && d == desc && a & (ACC_STATIC | ACC_ABSTRACT) == 0)
                {
                    continue;
                }
                match best {
                    None => best = Some(o),
                    Some(b) if self.is_sub(o, b) => best = Some(o),
                    Some(b) if self.is_sub(b, o) => {}
                    Some(_) => ambiguous = true,
                }
            }
            let (Some(iface), false) = (best, ambiguous) else {
                continue;
            };
            // nsc names the trait's implementation through its `m$` static.
            // No static, no forwarder: a Java interface cannot declare these
            // at all, so this only ever fires for a Scala trait.
            let helper = format!("{name}$");
            let (params, ret) = match split_desc(desc) {
                Some(p) => p,
                None => continue,
            };
            let helper_desc = format!("(L{iface};{}{ret}", &params[1..]);
            let has_helper = self.info(iface).is_some_and(|i| {
                i.methods
                    .iter()
                    .any(|(n, d, a)| n == &helper && d == &helper_desc && a & ACC_STATIC != 0)
            });
            if !has_helper {
                continue;
            }
            out.push(Bridge {
                name: name.to_string(),
                desc: desc.to_string(),
                kind: BridgeKind::Static {
                    iface: iface.to_string(),
                    helper,
                    desc: helper_desc,
                },
            });
        }
    }
}
