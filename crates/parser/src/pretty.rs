#![allow(unused_must_use)]
use crate::ast::*;
use std::fmt::Write;

pub fn dump_tree(tree: &Tree) -> String {
    let mut s = String::new();
    dump_into(&mut s, tree, 0);
    s
}

fn dump_into(s: &mut String, t: &Tree, indent: usize) {
    let pad = "  ".repeat(indent);
    let ty = if t.ty.is_no_type() {
        String::new()
    } else {
        format!(" : {}", t.ty)
    };
    let _ = match &t.kind {
        TreeKind::Empty => writeln!(s, "{pad}<empty>{ty}"),
        TreeKind::PackageDef { pid, stats } => {
            writeln!(s, "{pad}PackageDef{ty}");
            dump_into(s, pid, indent + 1);
            for st in stats {
                dump_into(s, st, indent + 1);
            }
            Ok(())
        }
        TreeKind::Import { expr, .. } => {
            writeln!(s, "{pad}Import{ty}");
            dump_into(s, expr, indent + 1);
            Ok(())
        }
        TreeKind::ClassDef {
            name,
            tparams,
            vparamss,
            impl_,
            mods,
            ..
        } => {
            let kind = if mods.flags.contains(Flags::TRAIT) {
                "Trait"
            } else if mods.flags.contains(Flags::CASE) {
                "CaseClass"
            } else {
                "Class"
            };
            writeln!(s, "{pad}{kind} {name}{ty}");
            for tp in tparams {
                dump_into(s, tp, indent + 1);
            }
            for (i, cl) in vparamss.iter().enumerate() {
                let _ = writeln!(s, "{pad}  paramss[{i}]");
                for p in cl {
                    dump_into(s, p, indent + 2);
                }
            }
            for p in &impl_.parents {
                dump_into(s, p, indent + 1);
            }
            for b in &impl_.body {
                dump_into(s, b, indent + 1);
            }
            Ok(())
        }
        TreeKind::ModuleDef { name, impl_, .. } => {
            writeln!(s, "{pad}Module {name}{ty}");
            for p in &impl_.parents {
                dump_into(s, p, indent + 1);
            }
            for b in &impl_.body {
                dump_into(s, b, indent + 1);
            }
            Ok(())
        }
        TreeKind::ValDef {
            name,
            tpt,
            rhs,
            mods,
            ..
        } => {
            let kw = if mods.flags.contains(Flags::MUTABLE) {
                "var"
            } else {
                "val"
            };
            writeln!(s, "{pad}ValDef {kw} {name}{ty}");
            if !tpt.is_empty() {
                dump_into(s, tpt, indent + 1);
            }
            if !rhs.is_empty() {
                dump_into(s, rhs, indent + 1);
            }
            Ok(())
        }
        TreeKind::DefDef {
            name,
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            writeln!(s, "{pad}DefDef {name}{ty}");
            for tp in tparams {
                dump_into(s, tp, indent + 1);
            }
            for cl in vparamss {
                for p in cl {
                    dump_into(s, p, indent + 1);
                }
            }
            if !tpt.is_empty() {
                dump_into(s, tpt, indent + 1);
            }
            if !rhs.is_empty() {
                dump_into(s, rhs, indent + 1);
            }
            Ok(())
        }
        TreeKind::TypeDef { name, rhs, views, ctx_bounds, mods, .. } => {
            let var = if mods.flags.contains(Flags::COVARIANT) {
                "+"
            } else if mods.flags.contains(Flags::CONTRAVARIANT) {
                "-"
            } else {
                ""
            };
            writeln!(s, "{pad}TypeDef {var}{name}{ty}");
            if !rhs.is_empty() {
                dump_into(s, rhs, indent + 1);
            }
            for v in views {
                writeln!(s, "{pad}  view");
                dump_into(s, v, indent + 1);
            }
            for c in ctx_bounds {
                writeln!(s, "{pad}  ctx");
                dump_into(s, c, indent + 1);
            }
            Ok(())
        }
        TreeKind::Block { stats, expr } => {
            writeln!(s, "{pad}Block{ty}");
            for st in stats {
                dump_into(s, st, indent + 1);
            }
            dump_into(s, expr, indent + 1);
            Ok(())
        }
        TreeKind::If {
            cond, thenp, elsep, ..
        } => {
            writeln!(s, "{pad}If{ty}");
            dump_into(s, cond, indent + 1);
            dump_into(s, thenp, indent + 1);
            dump_into(s, elsep, indent + 1);
            Ok(())
        }
        TreeKind::Match { selector, cases } => {
            writeln!(s, "{pad}Match{ty}");
            dump_into(s, selector, indent + 1);
            for c in cases {
                let _ = writeln!(s, "{pad}  CaseDef");
                dump_into(s, &c.pat, indent + 2);
                dump_into(s, &c.body, indent + 2);
            }
            Ok(())
        }
        TreeKind::Function { vparams, body } => {
            writeln!(s, "{pad}Function{ty}");
            for v in vparams {
                dump_into(s, v, indent + 1);
            }
            dump_into(s, body, indent + 1);
            Ok(())
        }
        TreeKind::Assign { lhs, rhs } => {
            writeln!(s, "{pad}Assign{ty}");
            dump_into(s, lhs, indent + 1);
            dump_into(s, rhs, indent + 1);
            Ok(())
        }
        TreeKind::While { cond, body } => {
            writeln!(s, "{pad}While{ty}");
            dump_into(s, cond, indent + 1);
            dump_into(s, body, indent + 1);
            Ok(())
        }
        TreeKind::DoWhile { body, cond } => {
            writeln!(s, "{pad}DoWhile{ty}");
            dump_into(s, body, indent + 1);
            dump_into(s, cond, indent + 1);
            Ok(())
        }
        TreeKind::Return { expr } => {
            writeln!(s, "{pad}Return{ty}");
            dump_into(s, expr, indent + 1);
            Ok(())
        }
        TreeKind::Throw { expr } => {
            writeln!(s, "{pad}Throw{ty}");
            dump_into(s, expr, indent + 1);
            Ok(())
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            writeln!(s, "{pad}Try{ty}");
            dump_into(s, block, indent + 1);
            for c in catches {
                dump_into(s, &c.body, indent + 1);
            }
            if !finalizer.is_empty() {
                dump_into(s, finalizer, indent + 1);
            }
            Ok(())
        }
        TreeKind::New { tpt } => {
            writeln!(s, "{pad}New{ty}");
            dump_into(s, tpt, indent + 1);
            Ok(())
        }
        TreeKind::Typed { expr, tpt } => {
            writeln!(s, "{pad}Typed{ty}");
            dump_into(s, expr, indent + 1);
            dump_into(s, tpt, indent + 1);
            Ok(())
        }
        TreeKind::TypeApply { fun, args } => {
            writeln!(s, "{pad}TypeApply{ty}");
            dump_into(s, fun, indent + 1);
            for a in args {
                dump_into(s, a, indent + 1);
            }
            Ok(())
        }
        TreeKind::Apply { fun, args } => {
            writeln!(s, "{pad}Apply{ty}");
            dump_into(s, fun, indent + 1);
            for a in args {
                dump_into(s, a, indent + 1);
            }
            Ok(())
        }
        TreeKind::Select { qual, name } => {
            writeln!(s, "{pad}Select {name}{ty}");
            dump_into(s, qual, indent + 1);
            Ok(())
        }
        TreeKind::Ident { name } => writeln!(s, "{pad}Ident {name}{ty}"),
        TreeKind::Literal { lit } => writeln!(s, "{pad}Literal {lit}{ty}"),
        TreeKind::This { .. } => writeln!(s, "{pad}This{ty}"),
        TreeKind::Super { .. } => writeln!(s, "{pad}Super{ty}"),
        TreeKind::Wildcard => writeln!(s, "{pad}_"),
        TreeKind::Bind { name, body } => {
            writeln!(s, "{pad}Bind {name}{ty}");
            dump_into(s, body, indent + 1);
            Ok(())
        }
        TreeKind::Star { elem } => {
            writeln!(s, "{pad}Star");
            dump_into(s, elem, indent + 1);
            Ok(())
        }
        TreeKind::Alternative { trees } => {
            writeln!(s, "{pad}Alternative");
            for t in trees {
                dump_into(s, t, indent + 1);
            }
            Ok(())
        }
        TreeKind::UnApply { fun, args } => {
            writeln!(s, "{pad}UnApply{ty}");
            dump_into(s, fun, indent + 1);
            for a in args {
                dump_into(s, a, indent + 1);
            }
            Ok(())
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            writeln!(s, "{pad}AppliedType{ty}");
            dump_into(s, tpt, indent + 1);
            for a in args {
                dump_into(s, a, indent + 1);
            }
            Ok(())
        }
        TreeKind::SingletonTypeTree { ref_ } => {
            writeln!(s, "{pad}SingletonType{ty}");
            dump_into(s, ref_, indent + 1);
            Ok(())
        }
        TreeKind::AnnotatedTypeTree { tpt, annot } => {
            writeln!(s, "{pad}AnnotatedType{ty}");
            dump_into(s, tpt, indent + 1);
            dump_into(s, annot, indent + 1);
            Ok(())
        }
        TreeKind::SelectFromTypeTree { qual, name, hash } => {
            let op = if *hash { "#" } else { "." };
            writeln!(s, "{pad}SelectFromType {op}{name}{ty}");
            dump_into(s, qual, indent + 1);
            Ok(())
        }
        TreeKind::CompoundTypeTree { parents, refinements } => {
            writeln!(s, "{pad}CompoundType{ty}");
            for p in parents {
                dump_into(s, p, indent + 1);
            }
            for d in refinements {
                dump_into(s, d, indent + 1);
            }
            Ok(())
        }
        TreeKind::ExistentialTypeTree { tpt, .. } => {
            writeln!(s, "{pad}Existential{ty}");
            dump_into(s, tpt, indent + 1);
            Ok(())
        }
        TreeKind::InterpolatedString { prefix, args, .. } => {
            writeln!(s, "{pad}Interpolate {prefix}{ty}");
            for a in args {
                dump_into(s, a, indent + 1);
            }
            Ok(())
        }
        TreeKind::Unimplemented { what } => writeln!(s, "{pad}Unimplemented({what})"),
        TreeKind::LabelDef { name, rhs, .. } => {
            writeln!(s, "{pad}LabelDef {name}{ty}");
            dump_into(s, rhs, indent + 1);
            Ok(())
        }
    };
}
