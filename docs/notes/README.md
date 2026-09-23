# Design notes

Focused notes on mechanisms that are easy to get wrong and are still true of
the code. Work logs, handoffs and batch records are not kept here; they live
in the git history.

- [Type prefixes for inner classes](prefix-types-design.md) — `crates/typer/src/prefix.rs`
- [Hard inference roots](hard-inference-design.md) — GADT bounds, lenient prototypes, lambda results, retracted `Nothing`
- [The reflect API and `reify` expansion](macro-reflect-and-reify.md)
- [`reify` over the typed body](reify-design.md) — the type reifier
- [Class-owned specialization oracle](specialization-class-oracle.md) — `tests/specialization_class_oracle.sh`
