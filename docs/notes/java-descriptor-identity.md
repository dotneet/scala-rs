# Exact class identity in Java descriptors (unaccepted candidate)

Parent implementation on `codex/descriptor-class-identity`, based on d2efbcbc.
`parse_field_ty_java` formerly routed descriptors under `scala/` and in the
unnamed package through simple-name lookup. A javac provider whose return and
field types were `scala.custom.List` consequently failed in scala-rs while the
same consumer compiled and ran with Scala 2.13.16. The loader now retains the
exact binary identity with `find_or_stub_java_class`. JVM representations of
String, Object, BoxedUnit, Nothing and Null retain their dedicated mappings.

The new Java provider / Scala consumer test covers scala.custom.List, String,
FunctionThing and an unnamed-package String, including both fields and method
results. Both compilers produce identical output under java -Xverify:all. A
real scala.collection.immutable.List passed to the custom List parameter is
rejected by both at Bad.scala:2. This does not establish that every legacy
ScalaSignature descriptor reader is correct; that path is unchanged.

Validation with pinned Temurin 17, UTF-8 and two Cargo jobs:

- Initial focused gate: 50 passed, 0 failed (java_descriptor_identity,
  cpvalueclass, javanest, nullcross, unitbox).
- Extended unnamed-package fixture: 1 passed, 0 failed.
- Compared to d2efbcbc: cats 351 errors / 81 files unchanged; GitBucket
  902 / 112 unchanged; Slick 0 errors / 1490 classes unchanged.
- Scala library: 1880 / 203 -> 1856 / 203. MainNode class identity and the
  TNode/CNode/LNode pattern compatibility diagnostics improve. Newly exposed
  generic inference diagnostics still need investigation; counts alone are
  not acceptance evidence.
- Accepted main remains e12cbdb2 plus documentation commit bc8c7700: cats
  350 / 81, GitBucket 912 / 111, Scala library 1613 / 171. The combined parent
  candidate is still unaccepted and has outstanding diagnostic differences.

Evidence: /tmp/scala-rs-codex/integration/java-name-probe/, especially
identity-repaired.json, identity-focused.log, default-package-focused.log and
measures/results.json plus each compile log. Full workspace / corpus / JVM
and Slick execution gates for this exact candidate remain pending.
