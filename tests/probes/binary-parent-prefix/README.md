# Binary parent prefix investigation

Base: main 57accf2b, accepted implementation 218b5340. No compiler fix yet.
This investigation began with the remaining gitbucket Shape diagnostics.
The hypothesis that a simple joinLeft would reproduce them was false: an
explicitly typed TableQuery constructor and joinLeft compile. Executing that
program instead exposed a missing enclosing constructor argument.

Lib.scala and Client.scala reproduce the failure without Slick. Compile Lib
with real scalac 2.13.16, then compile Client separately against its output.
Both compilers accept Client. Under java -Xverify:all the scalac client prints
7; the accepted scala-rs binary throws VerifyError in Child.<init>.
The constructor descriptor is (Lprobe/Owner;I)V, but the emitted stack contains
only uninitializedThis and integer 7. The required probe.O module is absent.
The binary used was .worktrees/codex-lazyzip-origin/target/release/scala-rs.
Logs are /tmp/scala-rs-shape-join/alias-{nsc,before}.{compile,run}.

The larger Slick case has the same failure in Rows.<init>: its descriptor
requires RelationalProfile before Tag and String, but only Tag and String
are emitted. The scalac version executes and generates a left outer join SQL
statement; the scala-rs version fails JVM verification before SQL generation.
Logs: /tmp/scala-rs-shape-join/{nsc,before}.{stdout,stderr}.

Relevant implementation paths: backend gen_desc::parent_super_ctor correctly
retains the JVM descriptor. The superclass call emission in gen_class separately asks
outer_field_class, whose enclosing_instance declines binary JAVA classes.
This is a mechanism hypothesis, not a finished diagnosis: a correction must
also preserve the actual imported type-alias prefix (O), not guess it from the
owner class. Two different instances of the same owner must remain distinct.
Explicit inherited member-type lookup without an alias currently fails earlier
with type Base is not a member of object O; it is a separate unclosed issue.

Next: trace parent trees and alias-prefix metadata before erasure; compare
source and binary libraries, two owner instances, static nested classes and
negative invalid-prefix cases. Execute new cases and compare stdout bytes to
scalac. Do not merge a compiler change without the full composed gate. No full
gate or new before measurement was run for this investigation.
