//! Parameter names for the hand-written prelude's methods.
//!
//! A member `PickleSupply` installs from a `ScalaSignature` gets a parameter
//! *symbol* per parameter, carrying the name nsc pickled. `prelude::method`
//! builds only a `Type::Method`, so a prelude method has parameter types and
//! no parameter symbols at all -- 1396 of the 1546 prelude methods that take
//! parameters, measured against the symbol table `install_prelude` builds.
//!
//! Member lookup finds the hand-written member first (`supply_from_pickle`
//! runs only when nothing matched), so this is what a named argument on a
//! standard-library method hits, in `--scala-library` mode as much as in the
//! private-runtime one: `List(1,2,3).mkString(sep = "-")` was "unimplemented
//! syntax: named arguments (method parameters not resolved)", while
//! `List(1,2,3).padTo(len = 5, elem = 0)` -- a member the prelude does not
//! declare, so it comes from the pickle -- compiled.
//!
//! The names are read back from the same pickle, on demand, for the overload
//! whose arity matches. Only the names: the parameter symbols are typed from
//! the prelude's own declaration, and they are kept beside the symbol table
//! rather than installed on the method, so nothing outside the named-argument
//! path can see them and no call is typed or emitted differently.
//!
//! Without a library jar there is no pickle to read, so this supplies nothing
//! and `reorder_named_args` reports its diagnostic exactly as before.

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::{Flags, SymbolId, Type};

/// `x$1` — the name `prelude_seq::poly_in` gives a parameter it has no name
/// for. No source can write it, so it never matches a named argument.
fn is_placeholder_name(n: &str) -> bool {
    n.strip_prefix("x$")
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

impl Typer {
    /// Parameter symbols for a prelude method that has none, or `None` when
    /// the library cannot name them.
    ///
    /// Memoised per method: the symbols are allocated once, owned by the
    /// method (as `PickleSupply::install` owns the ones it makes), so a second
    /// named-argument call on the same method reuses them.
    pub(crate) fn prelude_param_clauses(&mut self, m: SymbolId) -> Option<&[Vec<SymbolId>]> {
        if !self.wants_prelude_params(m) {
            return None;
        }
        if !self.prelude_params.contains_key(&m.0) {
            let built = self.build_prelude_params(m);
            self.prelude_params.insert(m.0, built);
        }
        match self.prelude_params.get(&m.0) {
            Some(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    }

    /// Whether `m` is a prelude method whose parameters have no name a source
    /// could write. Cheap enough to run on every candidate of an overloaded
    /// callee.
    ///
    /// Two shapes qualify. Most prelude methods have no parameter symbols at
    /// all. The 150 built by `prelude_seq::poly_in` have them, named `x$1`,
    /// `x$2`, … -- a placeholder, not the library's name, so `List(1).map(f =
    /// g)` was "unknown parameter name: f" rather than the missing-symbol
    /// diagnostic. Both are the same gap.
    fn wants_prelude_params(&self, m: SymbolId) -> bool {
        if !self.library_abi || m.is_none() || m.0 >= self.st.prelude_end {
            return false;
        }
        let s = self.st.get(m);
        if s.kind != SymKind::Method
            || !matches!(&s.ty, Type::Method { paramss, .. } if paramss.iter().any(|c| !c.is_empty()))
        {
            return false;
        }
        let declared: Vec<SymbolId> = if s.paramss.is_empty() {
            s.params.clone()
        } else {
            s.paramss.iter().flatten().copied().collect()
        };
        declared.is_empty()
            || declared
                .iter()
                .any(|&p| is_placeholder_name(&self.st.get(p).name))
    }

    /// Read the names out of the pickle and allocate one symbol per parameter.
    /// An empty answer is memoised too, so a miss costs one pickle walk.
    fn build_prelude_params(&mut self, m: SymbolId) -> Vec<Vec<SymbolId>> {
        let none = Vec::new();
        let s = self.st.get(m);
        let Type::Method { paramss, .. } = &s.ty else {
            return none;
        };
        let shape: Vec<Vec<Type>> = paramss.clone();
        let arity: usize = shape.iter().map(|c| c.len()).sum();
        let name = s.name.clone();
        let owner = s.owner;
        if owner.is_none() || !self.st.get(owner).is_class_like() {
            return none;
        }
        let internal = self.st.get(owner).jvm_name.clone();
        // Only the standard library: those are the pickles the prelude's
        // hand-written declarations stand in for.
        if !internal.starts_with("scala/") {
            return none;
        }
        let is_module = self.st.get(owner).kind == SymKind::ModuleClass;
        // nsc keeps operator names encoded in the pickle (`$amp` for `&`).
        let member = scala_rs_pickle::names::encode_method_name(&name);
        let Some(names) =
            self.pickle
                .pickled_param_names(&mut self.binary, &internal, is_module, &member, arity)
        else {
            return none;
        };
        if names.len() != arity {
            return none;
        }
        let mut out: Vec<Vec<SymbolId>> = Vec::with_capacity(shape.len());
        let mut next = names.into_iter();
        for clause in &shape {
            let mut ids = Vec::with_capacity(clause.len());
            for ty in clause {
                let Some(pname) = next.next() else {
                    return none;
                };
                let id = self.st.alloc(&pname, m, SymKind::Term, Flags::PARAM, "");
                // A repeated parameter's *symbol* has the type it has inside
                // the body, which is what every other producer of parameter
                // symbols records; `first_clause_of` reads `Repeated` off the
                // method type instead.
                self.st.get_mut(id).ty = match ty {
                    Type::Repeated(inner) => Type::Class {
                        sym: self.st.list_sym,
                        args: vec![(**inner).clone()],
                    },
                    other => other.clone(),
                };
                ids.push(id);
            }
            out.push(ids);
        }
        out
    }

    /// Whether `ids` declares every name the call site wrote.
    pub(crate) fn ids_cover_named(&self, ids: &[SymbolId], named: &[(String, Type)]) -> bool {
        named.iter().all(|(n, _)| {
            ids.iter()
                .any(|i| self.st.get(*i).name.as_str() == n.as_str())
        })
    }

    /// The same, straight off the argument trees.
    pub(crate) fn named_args_covered_by(
        &self,
        ids: &[SymbolId],
        args: &[scala_rs_parser::Tree],
    ) -> bool {
        args.iter()
            .filter_map(Self::named_arg_parts)
            .all(|(n, _)| ids.iter().any(|i| self.st.get(*i).name == n))
    }

    /// One overloaded alternative's first clause, when the library names its
    /// parameters and those names cover every name the call site wrote.
    ///
    /// The same test `alt_for_named_args` makes, restricted to *covers*: an
    /// alternative whose names do not include one the caller used cannot be
    /// the one meant, and there is no reason to prefer a wrong one here --
    /// `alt_for_named_args` has already declined, so the alternative is either
    /// right or the diagnostic stands.
    pub(crate) fn prelude_alt_clause(
        &mut self,
        m: SymbolId,
        nargs: usize,
        named: &[(String, Type)],
    ) -> Option<(Vec<SymbolId>, bool)> {
        if self.st.get(m).kind != SymKind::Method {
            return None;
        }
        let repeated = match &self.st.get(m).ty {
            Type::Method { paramss, .. } => paramss
                .first()
                .and_then(|c| c.last())
                .is_some_and(|t| matches!(t, Type::Repeated(_))),
            _ => false,
        };
        let ids = self.prelude_param_clauses(m)?.first()?.clone();
        if ids.len() < nargs && !repeated {
            return None;
        }
        let covers = named.iter().all(|(n, _)| {
            ids.iter()
                .any(|i| self.st.get(*i).name.as_str() == n.as_str())
        });
        covers.then_some((ids, repeated))
    }

    /// The clause of `m` a call with `remaining` clauses left to apply is
    /// filling, using the names read from the pickle.
    pub(crate) fn prelude_clause_for(
        &mut self,
        m: SymbolId,
        remaining: Option<usize>,
    ) -> Vec<SymbolId> {
        let Some(clauses) = self.prelude_param_clauses(m) else {
            return Vec::new();
        };
        match remaining {
            Some(n) if n < clauses.len() => clauses[clauses.len() - n].clone(),
            _ => clauses.first().cloned().unwrap_or_default(),
        }
    }
}
