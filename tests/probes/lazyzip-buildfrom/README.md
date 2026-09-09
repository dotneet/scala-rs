# Accepted value-class bridge slice

Composed commit `598ceef1` passed the full gate with corpus losses=0;
main contains the exact tested implementation. See tests/BASELINE.md and
/tmp/scala-rs-gate-598ceef1-codex/gate.log. The historical notes below describe
intermediate hypotheses and rejected states; the final gate supersedes their
"not fixed" and "not ready" status statements. The cats BuildFrom investigation
remains open.

# LazyZip / value-class investigation (not fixed)

Reference implementation: 9f3cae13 (db059c9b changes records only).
Real compiler: Scala 2.13.16; executions use java -Xverify:all.

The four cats BuildFrom[Iterable, ..., C] errors do not reproduce with a
standalone generic LazyList wrapper, even after requesting lazyZip on
scala.collection.Iterable or AbstractIterable. The generic wrapper executes
with stdout byte-identical to scalac (5 followed by a newline).

ValueClass.scala preserves the AnyVal wrapper and anonymous higher-kinded
instance shape. Both compilers accept it. scalac prints 5; scala-rs throws:

```
java.lang.ClassCastException: class Z cannot be cast to class scala.collection.immutable.LazyList
    at Z$$anon$1.ap(AnonNamed.scala)
```

This is a separate confirmed runtime defect, not yet an explanation for the
full cats BuildFrom diagnostics. No compiler fix or merge gate is claimed.
Probe outputs are in /tmp/lazyzip-probe/anon-named-{nsc,before}.out.
The before binary is .worktrees/codex-returning-elements/target/release/scala-rs.

An earlier probe named the local trait App and instead hit an
IncompatibleClassChangeError in scala.App.$init$. Renaming the trait to
ZipApplicative exposes the value-class failure above; the name collision
also remains unresolved.

## Local candidate (not gated)

The bridge now unboxes value-class arguments and boxes its result using
pre-erasure metadata. A second defect was at the call site: erase_apply
only unboxed generic results when the underlying representation was primitive.
It now also unboxes reference-backed value classes.

`cargo test --release -p scala-rs-cli --test vcbridge` passes (1 test),
comparing stdout bytes with scalac using java -Xverify:all. The LazyList
fixture runs in library ABI mode. A separate Int/String-backed value-class
fixture runs in both modes and prints `5` and `ok!`. The private runtime
rejects the LazyList fixture because those collection members are unavailable.

Remaining before acceptance: negative probes, broader value-class and bridge
regressions, generic underlying representations, full merge gate, records and
push. These changes are uncommitted and main remains db059c9b.

### Expanded checks

Related release suites valueclass, cpvalueclass, ifacebridge and anonbridge:
20 passed, zero failed (/tmp/lazyzip-probe/related.log). The new negative
fixture is rejected by scalac and both scala-rs ABI modes.

The new generic_value_class_bridge_matches_scalac test intentionally records
an unresolved failure: Wrapped[A](value: A), instantiated at String, prints
Wrapped@ddc! instead of ok!. Both accepted 9f3cae13 and this local candidate
have the defect; /tmp/lazyzip-probe/generic-before.out preserves the former.
The vcbridge suite currently has two passes and one failure (test3.log).

Disassembly identifies the next work: scalac emits apply(String):String plus
an Object bridge that calls Wrapped.value():Object and casts to String.
scala-rs emits only apply(Object):Object. erase_ty's value-class arm discards
actual type arguments and reads the mutable constructor field type; erase_symbols
has already erased that field's A to Object. Correcting this requires retaining
the declaration's underlying type before erasure, substituting actual arguments
for the unboxed method representation, and still using the erased declaration
signature for the wrapper constructor/accessor. The candidate bridge currently
uses the child representation as the wrapper descriptor; generic wrappers need
those descriptors separated too. Do not merge the current candidate.

### Generic underlying representation corrected; collision still open

The local candidate now snapshots value-class underlying declarations before
symbol erasure and substitutes the actual class arguments. Bridge constructor
and accessor descriptors use the declaration's erased field, independently of
the instantiated method type; box/unbox markers adapt the accessor result too.
String and Int instantiations pass in both ABI modes. The expanded run passed
23 tests (/tmp/lazyzip-probe/expanded.log).

A further probe at Wrapped[Any] finds an erased-descriptor collision. nsc emits
Main$$anon$$apply(Object):Object for the unboxed implementation and a separate
apply(Object):Object boxing bridge. scala-rs emits only the latter name and
prints the wrapper identity instead of any!. The generic fixture now includes
this case and expects ok!, 5, any! on separate lines; it remains unresolved.
This needs an implementation-name mapping used by definitions and call sites,
with the original name retained on the bridge; skipping equal descriptors is
not valid when the semantic representations differ. No gate has been run.

### Collision prototype

The local backend prototype gives an unboxed implementation a distinct name
and uses that name at definitions, invocations and bridge targets. Generic
call-result unboxing now uses declaration metadata, including the Object /
Object case. The vcbridge suite passes three tests (test8.log), including
String, Int and Any under both ABI modes. Any uses an identity body to isolate
the boxing boundary; Any.toString concatenation is separately unsupported by
the private runtime's current apply lowering.

The prototype is NOT ready for acceptance: real scalac rejects the public
named class in NamedCollision.scala with "bridge ... clashes with definition
of the member itself" but accepts the anonymous case via an expanded method
name. The current helper renames indiscriminately and uses an internal symbol
id suffix. It still needs correct visibility/owner eligibility, deterministic
ABI naming, and corresponding rejection tests, plus review of all invocation
paths (including super/mixin forwarding). This corrects the earlier assumption
that every equal-descriptor collision should be renamed.

### Anonymous-only collision rule

Checked real scalac and its saved Erasure.scala: EnterBridges.checkPair calls
resolveAnonymousBridgeClash only when member.owner.isAnonymousClass; other
clashes are reported. The candidate now rejects named implementations and
uses the anonymous owner's expanded name instead of a symbol-id suffix.
A new rejection test checks the diagnostic against scalac in both ABI modes.
Result-only collisions are covered by Source[Wrapped[Any]].get().

Latest runs: test9.log has 24 passes across valueclass, cpvalueclass,
ifacebridge, anonbridge and vcbridge; test10.log has 397 e2e and four vcbridge
passes (including the added result-only case). No full gate yet. Remaining:
review other call paths, binary generic value-class probes, clippy and full
composed gate before accepting this candidate.

### Binary accessor boundary

The new generic_value_class_from_scalac_binary test builds the value class
and generic traits with scalac, then compiles the client in both ABI modes.
It initially found a VerifyError: a constructor-field accessor supplied from
pickle is a separate method symbol, so erasure failed to recognize the read
as the underlying value and generated a nonexistent value$extension call.
The accessor check now recognizes the same-name nullary method belonging to
the constructor field's owner. test11.log records all five vcbridge tests
passing, with byte-identical runtime output from the binary-library client.
Clippy completed successfully; log: /tmp/lazyzip-probe/clippy.log.

### Candidate ready for composed gate

The reverse binary test also passes: scalac compiles and executes the client
against scala-rs-produced Wrapped[A], Transform[A] and Source[A] class files.
All six focused tests pass (test12.log). Clippy warnings compared by message
multiset against /tmp/returning-elements-clippy.log: 60 -> 59, no added messages.
The initial BuildFrom hypothesis was not established: this slice fixes an
independently reproduced silent value-class miscompile, not the four full-cats
BuildFrom diagnostics. Compilation counts must be measured by the merge gate.

## Corrections after rejected gate 70a2eaff

The composed gate failed: workspace 2658/3; corpus losses t10646, t13022,
t6385. Main records it at 8851cfa6; candidate code was not merged.

Four corrections recover the measured failures in focused execution:
- Nullary value-class extension calls box primitive receivers for the
  declaration's Object receiver slot, just like applied calls.
- A method declaring a value-class result returns its underlying value;
  instantiating that underlying T at Int requires ordinary primitive unboxing,
  not unboxing a value-class wrapper.
- The private ArrowAssoc tuple path boxes its receiver before Tuple2's Object
  constructor argument.
- Function parameter/result type arguments retain boxed value classes; lambda
  bodies box such results. A normal function differs from a SAM whose own
  declared result is a value class and uses the underlying ABI.

receiver3.log: 441 passes, zero failures across anonbridge, cpvalueclass, e2e,
incremental_forwarder, libprelude, ovl_exptype, valueclass, vcbridge. The three
lost corpus sources each compile and execute successfully with -Xverify:all
(/tmp/lazyzip-probe/{t10646,t13022,t6385}/run.log). The plain fixture now also
compares a value class passed into and returned from ordinary functions against
scalac in both ABI modes. These are focused results, not a new full gate.
