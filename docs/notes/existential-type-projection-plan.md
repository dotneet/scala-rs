# Existential bound and type projection follow-up

This slice preserves the declaration kind of aliases and writes the actual
upper bound of a class type parameter. It deliberately does not implement
type projections. The remaining nsc mismatch is visible in
`xmeta_existential_provider.scala`: nsc writes `E#Elem` as a `TYPEREFtpe`
whose prefix is the type `E`, while the current pickle writes a
`NOPREFIXtpe` reference to `Elem`. The same missing prefix makes
`ConcreteProfile.api.Item[String]` lose the `Schema` member's owner view.

The parser's `Type::TypeMember(SymbolId)` stores only the member symbol, so
the prefix is lost before pickling. A narrow follow-up should proceed in this
order:

1. Add a `TypeProjection { prefix: Box<Type>, member: SymbolId }` variant and
   create it only when typing a `#` projection or an equivalent member select.
   Keep plain `TypeMember` for an unqualified abstract member so existing
   alias and refinement behavior remains stable during migration.
2. Thread the new variant through substitution and as-seen-from operations:
   `subst_map`, `subst_refine_aliases`, `expand_type_members`, `dealias`,
   `is_sub_type`, and the bounded-member traversal. Substitution must recurse
   into the prefix while retaining the member symbol; erasure should erase the
   projected member's resolved upper bound, with the prefix used only for
   lookup.
3. In the backend, emit the prefix type reference before the projected member
   symbol in `TYPEREFtpe`. The existing alias/abstract declaration tag logic
   must stay independent of this change.
4. Validate with the two existing nsc consumers: the `E#Elem` assignment in
   `xmeta_existential_consumer.scala` and the `Schema` method call in
   `xmeta_profile_consumer.scala`; compare their pickles against nsc before
   changing erasure or JVM signatures.

Alias resolution needs an explicit owner environment in that follow-up. For
`ConcreteProfile.api.Item[A] = Schema`, the alias is owned by `api` but its
RHS is declared by `BasicProfile`; resolving the RHS in the consumer's bare
scope can turn it into an unqualified member. Imported aliases currently pass
through `pickled_alias_type`, so the projection prefix and the declaring owner
must survive that installation and later `as_seen_from` substitution.

The consumer matrix should also retain these subtype counterexamples while the
new variant is threaded through the typer:

- `Pair[Any, String]` must be rejected for `class Pair[A <: B, B]`, while
  `Pair[String, Any]` is accepted. This catches losing a forward-reference
  upper bound or accidentally treating the two parameters as independent.
- `FBound[Int]` must be rejected for `A <: Comparable[A]`; the self reference
  must be substituted under the bound before subtyping.
- `LowerBound[Nothing]` must be rejected while `LowerBound[Any]` is accepted
  for `A >: String`. Lower bounds must remain present when a projected prefix
  is substituted.
- For a contravariant alias such as `type Alias[T] = Sink[T]` where
  `class Sink[-T]`, subtyping must preserve the variance of the expanded RHS;
  retaining only the member symbol would incorrectly make `Alias[Any]` and
  `Alias[String]` invariant. For projected members, compare the resolved
  member through its prefix rather than comparing unrelated bare `TypeMember`
  symbols.

This plan is intentionally separate from the current bound fix because
changing `Type::TypeMember` itself would touch all type consumers and would
make alias-kind and existential-bound regressions harder to isolate.
