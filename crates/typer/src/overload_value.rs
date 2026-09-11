//! An overloaded reference read against a function type.
//!
//! nsc's `inferExprAlternative` keeps the alternatives compatible with the
//! expected type -- a method counts, through eta-expansion -- and then picks
//! the most specific. A member that is not a method is "as specific as" a
//! method outright, while the method is as specific as the value only if the
//! value is *applicable* to its parameters, which a plain function-typed value
//! is not in `Infer.isApplicable`. So the value wins:
//!
//! ```scala
//! def f(s: String): String = "1"
//! val f: String => String = s => "2"
//! val t: String => String = f   // the val: t("") == "2" (run/t9395)
//! ```
//!
//! scala-rs kept the set and eta-expanded the method.

use crate::check::Typer;
use scala_rs_parser::ast::Type;
use scala_rs_parser::SymbolId;

impl Typer {
    /// The one non-method alternative whose type conforms to the function
    /// type `pt`, if there is exactly one.
    pub(crate) fn function_value_alternative(
        &self,
        alts: &[(SymbolId, Type)],
        pt: &Type,
    ) -> Option<SymbolId> {
        if !matches!(pt, Type::Function { .. }) {
            return None;
        }
        let mut fits = alts.iter().filter(|(_, t)| {
            !matches!(t, Type::Method { .. } | Type::Overload(_) | Type::NoType)
                && self.st.is_sub_type(t, pt)
        });
        match (fits.next(), fits.next()) {
            (Some((id, _)), None) => Some(*id),
            _ => None,
        }
    }
}
