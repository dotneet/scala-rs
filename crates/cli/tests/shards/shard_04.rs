//! Aggregated CLI integration tests (alphabetical slice 4/8).
#![allow(clippy::duplicate_mod)] // Legacy fixtures intentionally share helper modules.

#[path = "../support/mod.rs"]
mod support;

#[path = "../java_pattern_parent.rs"]
mod java_pattern_parent;
#[path = "../javanest.rs"]
mod javanest;
#[path = "../jvarargs.rs"]
mod jvarargs;
#[path = "../jvm_inaccessible_parent.rs"]
mod jvm_inaccessible_parent;
#[path = "../jwarm.rs"]
mod jwarm;
#[path = "../kernel.rs"]
mod kernel;
#[path = "../kindproj.rs"]
mod kindproj;
#[path = "../kvar.rs"]
mod kvar;
#[path = "../lang.rs"]
mod lang;
#[path = "../lastone.rs"]
mod lastone;
#[path = "../lasttwo.rs"]
mod lasttwo;
#[path = "../late_factory_inference.rs"]
mod late_factory_inference;
#[path = "../lazy_template.rs"]
mod lazy_template;
#[path = "../lazyref.rs"]
mod lazyref;
#[path = "../lazysig2.rs"]
mod lazysig2;
#[path = "../lazysig_impl2.rs"]
mod lazysig_impl2;
#[path = "../lcoll.rs"]
mod lcoll;
#[path = "../lconc.rs"]
mod lconc;
#[path = "../lexicalcontext.rs"]
mod lexicalcontext;
#[path = "../lf.rs"]
mod lf;
#[path = "../libapp.rs"]
mod libapp;
#[path = "../libctor.rs"]
mod libctor;
#[path = "../libdecl.rs"]
mod libdecl;
#[path = "../libmaxmin.rs"]
mod libmaxmin;
#[path = "../libnotype.rs"]
mod libnotype;
#[path = "../libov.rs"]
mod libov;
#[path = "../liboverload.rs"]
mod liboverload;
#[path = "../libprelude.rs"]
mod libprelude;
#[path = "../library_trait_bridges.rs"]
mod library_trait_bridges;
#[path = "../linearization.rs"]
mod linearization;
#[path = "../list_alias_implicit.rs"]
mod list_alias_implicit;
#[path = "../listcore_text.rs"]
mod listcore_text;
#[path = "../localcc.rs"]
mod localcc;
#[path = "../localconv.rs"]
mod localconv;
#[path = "../localtrait.rs"]
mod localtrait;
#[path = "../loopframe.rs"]
mod loopframe;
#[path = "../lowbound.rs"]
mod lowbound;
#[path = "../lsurf.rs"]
mod lsurf;
#[path = "../lz.rs"]
mod lz;
#[path = "../lz2.rs"]
mod lz2;
#[path = "../lzorigin.rs"]
mod lzorigin;
#[path = "../macromirror.rs"]
mod macromirror;
#[path = "../macros.rs"]
mod macros;
#[path = "../macrotag.rs"]
mod macrotag;
#[path = "../macrotransportbatch.rs"]
mod macrotransportbatch;
