//! Resume candidate fitting after a whitebox expansion determines its output.
//! The immutable solver records work; only the enclosing mutable call-site
//! transaction executes it. Neither inferred types nor trees escape that scope.
use super::*;

#[derive(Default)]
pub(crate) struct WhiteboxFits {
    frames: Vec<FitFrame>,
}
struct FitFrame {
    key: (usize, u32, u32, SymbolId, usize, Option<u64>),
    span: Span,
    entries: Vec<FitEntry>,
    pending_warm: Vec<Type>,
    warmed: super::TypeSet,
}
struct FitEntry {
    origin: SymbolId,
    base: Type,
    targs: Vec<Type>,
    depth: usize,
    open: Vec<(SymbolId, Type)>,
    building: Vec<(SymbolId, Type)>,
    context: Vec<(SymbolId, Type)>,
    attempted: bool,
    result: Option<Type>,
    tree: Option<Tree>,
}

impl Typer {
    pub(crate) fn with_whitebox_fits<R>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let key = (
            self.file_index,
            span.lo.0,
            span.hi.0,
            self.st.owner,
            self.st.scopes.len(),
            self.macro_context_stack.last().copied(),
        );
        let pushed = self
            .whitebox_fits
            .borrow()
            .frames
            .last()
            .is_none_or(|frame| frame.key != key);
        if pushed {
            self.whitebox_fits.borrow_mut().frames.push(FitFrame {
                key,
                span,
                entries: Vec::new(),
                pending_warm: Vec::new(),
                warmed: Default::default(),
            });
        }
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        if pushed {
            self.whitebox_fits.borrow_mut().frames.pop();
        }
        match out {
            Ok(out) => out,
            Err(error) => std::panic::resume_unwind(error),
        }
    }

    pub(crate) fn retry_whitebox_fits<R>(&mut self, mut search: impl FnMut(&mut Self) -> R) -> R {
        loop {
            let result = search(self);
            let warm = self
                .whitebox_fits
                .borrow_mut()
                .frames
                .last_mut()
                .map(|frame| std::mem::take(&mut frame.pending_warm))
                .unwrap_or_default();
            if !warm.is_empty() {
                for wanted in &warm {
                    self.warm_implicit_scope(wanted);
                }
                self.warm_implicit_candidates(&warm);
                self.invalidate_implicit_caches();
                continue;
            }
            let work = {
                let mut state = self.whitebox_fits.borrow_mut();
                state.frames.last_mut().and_then(|frame| {
                    frame
                        .entries
                        .iter_mut()
                        .enumerate()
                        .find(|(_, entry)| !entry.attempted)
                        .map(|(index, entry)| {
                            entry.attempted = true;
                            (
                                index,
                                entry.origin,
                                entry.base.clone(),
                                entry.depth,
                                entry.open.clone(),
                                entry.building.clone(),
                                frame.span,
                            )
                        })
                })
            };
            let Some((index, origin, base, depth, open, building, span)) = work else {
                return result;
            };
            let mark = self.diags.len();
            let key = self.macro_failure_key(span);
            let saved_open = self.open_implicits.replace(open);
            let saved_building = std::mem::replace(&mut self.building_implicits, building);
            let expanded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.with_isolated_macro_failure(key, |this| {
                    this.implicit_tree(origin, &base, span, depth)
                })
            }));
            self.open_implicits.replace(saved_open);
            self.building_implicits = saved_building;
            let (tree, failure) = match expanded {
                Ok(expanded) => expanded,
                Err(error) => std::panic::resume_unwind(error),
            };
            let errors = self.take_probe_errors(mark);
            if failure.is_none() && errors.is_none() && !tree.ty.is_error() && !tree.ty.is_no_type()
            {
                let mut state = self.whitebox_fits.borrow_mut();
                let entry = &mut state.frames.last_mut().unwrap().entries[index];
                entry.result = Some(tree.ty.clone());
                entry.tree = Some(tree);
            }
            self.invalidate_implicit_caches();
        }
    }

    pub(super) fn request_whitebox_warm(&self, wanted: &Type, open: &[SymbolId]) {
        let base = match wanted {
            Type::Refined { parents, .. } if parents.len() == 1 => &parents[0],
            other => other,
        };
        if !matches!(base, Type::Class { .. })
            || open
                .iter()
                .any(|tp| crate::check::type_mentions_tparam_deep(base, *tp))
        {
            return;
        }
        let mut state = self.whitebox_fits.borrow_mut();
        let Some(frame) = state.frames.last_mut() else {
            return;
        };
        // These concrete inputs did not exist before an expansion. Other
        // failed searches continue to use the ordinary classpath warm-up.
        if frame.entries.iter().any(|entry| entry.result.is_some()) && frame.warmed.insert(wanted) {
            frame.pending_warm.push(wanted.clone());
        }
    }

    /// Some(None) suspends this candidate; None leaves ordinary fitting alone.
    pub(super) fn fit_whitebox_expansion(
        &self,
        id: SymbolId,
        pt: &Type,
        undet: &[SymbolId],
        depth: usize,
    ) -> Option<Option<ImplicitFit>> {
        if self.implicit_macros_disabled
            || !self
                .st
                .get(id)
                .macro_impl
                .as_ref()
                .is_some_and(|m| !m.blackbox)
        {
            return None;
        }
        if matches!(&self.st.get(id).ty, Type::Method { paramss, .. } if paramss.iter().any(|clause| !clause.is_empty()))
            && !self.only_implicit_clauses(id)
        {
            return None;
        }
        let Type::Refined { parents, decls } = pt else {
            return None;
        };
        if parents.len() != 1 || decls.is_empty() || crate::prefix::view_prefix(pt).is_some() {
            return None;
        }
        let base = &parents[0];
        if undet
            .iter()
            .any(|tp| crate::check::type_mentions_tparam_deep(base, *tp))
        {
            return None;
        }
        let origin = self
            .implicit_instance_origins
            .get(&id)
            .copied()
            .unwrap_or(id);
        let building = self.whitebox_building_context();
        let context = self.whitebox_context(&building);
        let mut state = self.whitebox_fits.borrow_mut();
        let frame = state.frames.last_mut()?;
        if frame.key.0 != self.file_index
            || frame.key.3 != self.st.owner
            || frame.key.4 != self.st.scopes.len()
            || frame.key.5 != self.macro_context_stack.last().copied()
        {
            return None;
        }
        if let Some(entry) = frame
            .entries
            .iter()
            .find(|entry| entry.origin == origin && entry.base == *base && entry.context == context)
        {
            let Some(result) = entry.result.as_ref() else {
                return Some(None);
            };
            let mut unify = Unify::new(self, std::iter::empty(), undet.iter().copied());
            unify.allow_evidence_constructors();
            for parent in parents {
                let projected = self
                    .st
                    .class_sym_of(parent)
                    .and_then(|sym| self.base_type_instance(result, sym, 0));
                if !unify.unify(projected.as_ref().unwrap_or(result), parent) {
                    return Some(None);
                }
            }
            for decl in decls {
                if let scala_rs_parser::RefineDecl::Type {
                    name,
                    rhs: Some(wanted),
                    ..
                } = decl
                {
                    let Some(actual) = self.st.lookup_type_member_on(result, name) else {
                        return Some(None);
                    };
                    if !unify.unify(&actual, wanted) {
                        return Some(None);
                    }
                }
            }
            let bindings = undet
                .iter()
                .filter_map(|tp| unify.solved(*tp).map(|ty| (*tp, self.simplify_solved(&ty))))
                .collect::<Vec<_>>();
            let wanted = self.subst_undet(pt, &bindings);
            return Some(
                self.implicit_result_conforms(result, &wanted)
                    .then(|| ImplicitFit {
                        targs: entry.targs.clone(),
                        undet: bindings,
                    }),
            );
        }
        drop(state);
        if !undet
            .iter()
            .any(|tp| crate::check::type_mentions_tparam_deep(pt, *tp))
        {
            return None;
        }
        // Determine inputs and validate the macro's own evidence using the
        // ordinary solver. Nested suspended candidates are handled first.
        let fit = self.implicit_fit_at(id, base, depth, &[])?;
        if fit.targs.iter().any(|ty| {
            undet
                .iter()
                .any(|tp| crate::check::type_mentions_tparam_deep(ty, *tp))
        }) {
            return None;
        }
        if self.implicit_diverges(id, pt) {
            return Some(None);
        }
        let mut state = self.whitebox_fits.borrow_mut();
        state.frames.last_mut()?.entries.push(FitEntry {
            origin,
            base: base.clone(),
            targs: fit.targs,
            depth,
            open: self.open_implicits.borrow().clone(),
            building,
            context,
            attempted: false,
            result: None,
            tree: None,
        });
        Some(None)
    }

    fn whitebox_building_context(&self) -> Vec<(SymbolId, Type)> {
        let mut building = self.building_implicits.clone();
        let open = self.open_implicits.borrow();
        let built_context = self.whitebox_context(&building);
        let open_context = self.whitebox_context(&open);
        // Materialization fits the current rule again while that rule is
        // already on the building stack. The common suffix/prefix denotes
        // the same live candidates, not a second recursive application.
        let overlap = (0..=building.len().min(open.len()))
            .rev()
            .find(|&n| built_context[building.len() - n..] == open_context[..n])
            .unwrap_or(0);
        building.extend(open.iter().skip(overlap).cloned());
        building
    }

    fn whitebox_context(&self, open: &[(SymbolId, Type)]) -> Vec<(SymbolId, Type)> {
        open.iter()
            .map(|(id, pt)| {
                (
                    self.implicit_instance_origins
                        .get(id)
                        .copied()
                        .unwrap_or(*id),
                    pt.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn take_whitebox_tree(&mut self, id: SymbolId, pt: &Type) -> Option<Tree> {
        let origin = self
            .implicit_instance_origins
            .get(&id)
            .copied()
            .unwrap_or(id);
        let context = self.whitebox_context(&self.building_implicits);
        let mut state = self.whitebox_fits.borrow_mut();
        let frame = state.frames.last_mut()?;
        let entry = frame.entries.iter_mut().find(|entry| {
            entry.origin == origin
                && entry.context == context
                && entry
                    .result
                    .as_ref()
                    .is_some_and(|result| self.implicit_result_conforms(result, pt))
        })?;
        entry.tree.take()
    }
}
