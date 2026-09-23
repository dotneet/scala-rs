//! Aggregated CLI integration tests (alphabetical slice 1/8).
#![allow(clippy::duplicate_mod)] // Legacy fixtures intentionally share helper modules.
//!
//! The source files remain in `tests/` so fixture-relative paths and blame stay
//! stable.  Cargo's explicit targets below are the only thing that discovers
//! these modules once auto-discovery is disabled in `Cargo.toml`.

#[path = "../support/mod.rs"]
mod support;

#[path = "../absproj.rs"]
mod absproj;
#[path = "../absresult.rs"]
mod absresult;
#[path = "../abstract_pattern_bounds.rs"]
mod abstract_pattern_bounds;
#[path = "../abstract_val_classfile.rs"]
mod abstract_val_classfile;
#[path = "../accepttoomuch.rs"]
mod accepttoomuch;
#[path = "../aliaslookup.rs"]
mod aliaslookup;
#[path = "../ambigmap.rs"]
mod ambigmap;
#[path = "../annotation_import_owner.rs"]
mod annotation_import_owner;
#[path = "../anonbridge.rs"]
mod anonbridge;
#[path = "../anoncap.rs"]
mod anoncap;
#[path = "../anyconstr.rs"]
mod anyconstr;
#[path = "../appidentity.rs"]
mod appidentity;
#[path = "../applied_collection_names.rs"]
mod applied_collection_names;
#[path = "../applied_inner_result_prefix.rs"]
mod applied_inner_result_prefix;
#[path = "../arraygen.rs"]
mod arraygen;
#[path = "../arrconv.rs"]
mod arrconv;
#[path = "../asttype.rs"]
mod asttype;
#[path = "../auxpickle.rs"]
mod auxpickle;
#[path = "../backendtypes.rs"]
mod backendtypes;
#[path = "../bare_blocks.rs"]
mod bare_blocks;
#[path = "../basetype.rs"]
mod basetype;
#[path = "../batchtypes.rs"]
mod batchtypes;
#[path = "../binary_client_rules.rs"]
mod binary_client_rules;
#[path = "../binary_library_members.rs"]
mod binary_library_members;
#[path = "../binary_value_class_lambda.rs"]
mod binary_value_class_lambda;
#[path = "../boxed.rs"]
mod boxed;
#[path = "../bparent.rs"]
mod bparent;
#[path = "../btargs.rs"]
mod btargs;
#[path = "../btmeet.rs"]
mod btmeet;
#[path = "../bts.rs"]
mod bts;
#[path = "../buildfrom.rs"]
mod buildfrom;
#[path = "../buildfrom2.rs"]
mod buildfrom2;
#[path = "../byname_followup.rs"]
mod byname_followup;
#[path = "../caseabi.rs"]
mod caseabi;
#[path = "../caseeq.rs"]
mod caseeq;
#[path = "../cats2.rs"]
mod cats2;
#[path = "../cats3.rs"]
mod cats3;
#[path = "../cats4.rs"]
mod cats4;
#[path = "../catseta.rs"]
mod catseta;
#[path = "../catsimpl.rs"]
mod catsimpl;
#[path = "../catsr.rs"]
mod catsr;
#[path = "../catstail.rs"]
mod catstail;
#[path = "../catstail3.rs"]
mod catstail3;
#[path = "../catsyntax.rs"]
mod catsyntax;
#[path = "../cfwd.rs"]
mod cfwd;
#[path = "../classtag_inference.rs"]
mod classtag_inference;
#[path = "../codegen_diag.rs"]
mod codegen_diag;
#[path = "../codegen_nested_generic.rs"]
mod codegen_nested_generic;
#[path = "../coll.rs"]
mod coll;
#[path = "../collectionresults.rs"]
mod collectionresults;
