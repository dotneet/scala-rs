//! Aggregated CLI integration tests (alphabetical slice 2/8).
#![allow(clippy::duplicate_mod)] // Legacy fixtures intentionally share helper modules.

#[path = "../support/mod.rs"]
mod support;

#[path = "../companionkind.rs"]
mod companionkind;
#[path = "../conform.rs"]
mod conform;
#[path = "../conspat.rs"]
mod conspat;
#[path = "../contextualfollowup.rs"]
mod contextualfollowup;
#[path = "../contextualinference.rs"]
mod contextualinference;
#[path = "../conversion_inference.rs"]
mod conversion_inference;
#[path = "../convimpl.rs"]
mod convimpl;
#[path = "../cpvalueclass.rs"]
mod cpvalueclass;
#[path = "../ctoraccessor.rs"]
mod ctoraccessor;
#[path = "../ctorgaps.rs"]
mod ctorgaps;
#[path = "../ctorstmt.rs"]
mod ctorstmt;
#[path = "../ctxev.rs"]
mod ctxev;
#[path = "../cyclic.rs"]
mod cyclic;
#[path = "../czero.rs"]
mod czero;
#[path = "../dbconfigforurl.rs"]
mod dbconfigforurl;
#[path = "../dbio.rs"]
mod dbio;
#[path = "../deadcode.rs"]
mod deadcode;
#[path = "../declbound.rs"]
mod declbound;
#[path = "../declvsdef.rs"]
mod declvsdef;
#[path = "../default_type_imports.rs"]
mod default_type_imports;
#[path = "../defaultargs.rs"]
mod defaultargs;
#[path = "../dependentadaptation.rs"]
mod dependentadaptation;
#[path = "../dirsig.rs"]
mod dirsig;
#[path = "../durrange.rs"]
mod durrange;
#[path = "../e2e.rs"]
mod e2e;
#[path = "../earlyscope.rs"]
mod earlyscope;
#[path = "../eithertry.rs"]
mod eithertry;
#[path = "../eithert_widen.rs"]
mod eithert_widen;
#[path = "../elemtype.rs"]
mod elemtype;
#[path = "../engine.rs"]
mod engine;
#[path = "../eqtail.rs"]
mod eqtail;
#[path = "../erascg.rs"]
mod erascg;
#[path = "../erasure3.rs"]
mod erasure3;
#[path = "../evidencebatch.rs"]
mod evidencebatch;
#[path = "../existential.rs"]
mod existential;
#[path = "../fewerclasses.rs"]
mod fewerclasses;
#[path = "../fieldscope.rs"]
mod fieldscope;
#[path = "../fillconcat.rs"]
mod fillconcat;
#[path = "../final1.rs"]
mod final1;
#[path = "../final2.rs"]
mod final2;
#[path = "../final3.rs"]
mod final3;
#[path = "../formnamesbatch.rs"]
mod formnamesbatch;
#[path = "../function_pattern.rs"]
mod function_pattern;
#[path = "../fvg.rs"]
mod fvg;
