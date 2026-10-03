//! Aggregated CLI integration tests (alphabetical slice 7/8).
#![allow(clippy::duplicate_mod)] // Legacy fixtures intentionally share helper modules.

#[path = "../support/mod.rs"]
mod support;

#[path = "../fs2_io.rs"]
mod fs2_io;
#[path = "../samconv.rs"]
mod samconv;
#[path = "../scalac_client_pickle.rs"]
mod scalac_client_pickle;
#[path = "../scalalibmisc.rs"]
mod scalalibmisc;
#[path = "../secondaryctor.rs"]
mod secondaryctor;
#[path = "../selfprefix.rs"]
mod selfprefix;
#[path = "../selfrec.rs"]
mod selfrec;
#[path = "../selfwild.rs"]
mod selfwild;
#[path = "../seq_default_args.rs"]
mod seq_default_args;
#[path = "../seqfn.rs"]
mod seqfn;
#[path = "../seqpat.rs"]
mod seqpat;
#[path = "../seqpickle.rs"]
mod seqpickle;
#[path = "../setapply.rs"]
mod setapply;
#[path = "../setmap.rs"]
mod setmap;
#[path = "../shapeless_hlist_ops.rs"]
mod shapeless_hlist_ops;
#[path = "../sibover.rs"]
mod sibover;
#[path = "../signature.rs"]
mod signature;
#[path = "../singleton.rs"]
mod singleton;
#[path = "../singleton_metadata.rs"]
mod singleton_metadata;
#[path = "../slickddl.rs"]
mod slickddl;
#[path = "../slickddl_classpath.rs"]
mod slickddl_classpath;
#[path = "../slickimpl.rs"]
mod slickimpl;
#[path = "../slickimplicit.rs"]
mod slickimplicit;
#[path = "../slickoptionmapper.rs"]
mod slickoptionmapper;
#[path = "../slickparse.rs"]
mod slickparse;
#[path = "../slickrun.rs"]
mod slickrun;
#[path = "../slickrun3.rs"]
mod slickrun3;
#[path = "../slickshape.rs"]
mod slickshape;
#[path = "../smallgaps.rs"]
mod smallgaps;
#[path = "../sortedlb.rs"]
mod sortedlb;
#[path = "../sortedmap.rs"]
mod sortedmap;
#[path = "../specialized.rs"]
mod specialized;
#[path = "../specialized_tuple_fields.rs"]
mod specialized_tuple_fields;
#[path = "../sqlstoragebatch.rs"]
mod sqlstoragebatch;
#[path = "../stableprofile.rs"]
mod stableprofile;
#[path = "../stale_term_method.rs"]
mod stale_term_method;
#[path = "../stmtval.rs"]
mod stmtval;
#[path = "../strarrayops.rs"]
mod strarrayops;
#[path = "../string_generic_flat_map.rs"]
mod string_generic_flat_map;
#[path = "../stringops8.rs"]
mod stringops8;
#[path = "../subtypeterm.rs"]
mod subtypeterm;
#[path = "../super_call_class_outer.rs"]
mod super_call_class_outer;
#[path = "../super_object.rs"]
mod super_object;
#[path = "../sysout.rs"]
mod sysout;
#[path = "../tablequery_parent.rs"]
mod tablequery_parent;
#[path = "../tail1.rs"]
mod tail1;
#[path = "../tail2.rs"]
mod tail2;
#[path = "../tail3.rs"]
mod tail3;
#[path = "../trait_bridge_default.rs"]
mod trait_bridge_default;
#[path = "../trait_constant_val.rs"]
mod trait_constant_val;
#[path = "../trait_pattern_binder.rs"]
mod trait_pattern_binder;
#[path = "../unit_local_scope.rs"]
mod unit_local_scope;
#[path = "../withfilter_subtype.rs"]
mod withfilter_subtype;
