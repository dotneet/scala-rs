//! Aggregated CLI integration tests (alphabetical slice 6/8).
#![allow(clippy::duplicate_mod)] // Legacy fixtures intentionally share helper modules.

#[path = "../support/mod.rs"]
mod support;

#[path = "../ovl2.rs"]
mod ovl2;
#[path = "../ovl3.rs"]
mod ovl3;
#[path = "../ovl4.rs"]
mod ovl4;
#[path = "../ovl_exptype.rs"]
mod ovl_exptype;
#[path = "../parent_lambda.rs"]
mod parent_lambda;
#[path = "../parentcheck.rs"]
mod parentcheck;
#[path = "../parentimpl.rs"]
mod parentimpl;
#[path = "../parse.rs"]
mod parse;
#[path = "../partfactory.rs"]
mod partfactory;
#[path = "../patbind.rs"]
mod patbind;
#[path = "../pathdep.rs"]
mod pathdep;
#[path = "../pattern_acceptance.rs"]
mod pattern_acceptance;
#[path = "../pattern_type_binders.rs"]
mod pattern_type_binders;
#[path = "../pf_empty_inference.rs"]
mod pf_empty_inference;
#[path = "../pfx.rs"]
mod pfx;
#[path = "../pickle_lib.rs"]
mod pickle_lib;
#[path = "../pickleparams.rs"]
mod pickleparams;
#[path = "../pkgalias.rs"]
mod pkgalias;
#[path = "../pkgobjdup.rs"]
mod pkgobjdup;
#[path = "../preludefidelity.rs"]
mod preludefidelity;
#[path = "../preludelb.rs"]
mod preludelb;
#[path = "../preludeshadow.rs"]
mod preludeshadow;
#[path = "../process_tree.rs"]
mod process_tree;
#[path = "../product.rs"]
mod product;
#[path = "../proj.rs"]
mod proj;
#[path = "../proven.rs"]
mod proven;
#[path = "../qualfallback.rs"]
mod qualfallback;
#[path = "../qualifier_retry.rs"]
mod qualifier_retry;
#[path = "../quasi.rs"]
mod quasi;
#[path = "../reify.rs"]
mod reify;
#[path = "../reify2.rs"]
mod reify2;
#[path = "../reifydefs.rs"]
mod reifydefs;
#[path = "../reject.rs"]
mod reject;
#[path = "../resultprefix.rs"]
mod resultprefix;
#[path = "../retfamily.rs"]
mod retfamily;
#[path = "../rf.rs"]
mod rf;
#[path = "../rf_reify.rs"]
mod rf_reify;
#[path = "../rh.rs"]
mod rh;
#[path = "../rhf.rs"]
mod rhf;
#[path = "../rto.rs"]
mod rto;
#[path = "../rtprobe.rs"]
mod rtprobe;
#[path = "../rtv.rs"]
mod rtv;
#[path = "../runwrong.rs"]
mod runwrong;
#[path = "../runwrong3.rs"]
mod runwrong3;
