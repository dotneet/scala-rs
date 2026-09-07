// An `extends` graph that leads back into itself. Real scalac 2.13.16 rejects
// each cycle once, at the template that closes it, naming the class whose
// completion was re-entered:
//
//   linterm_cycle_bad.scala:15: error: illegal cyclic reference involving trait X
//   linterm_cycle_bad.scala:19: error: illegal cyclic reference involving class C
//   linterm_cycle_bad.scala:22: error: illegal cyclic reference involving trait S
//
// The first three lines are the whole reproduction of the hang this fixture
// was written for: two parents per node turn `lin.rs`'s recursion into a
// branching tree, and under the old depth-only bound it ran for hours at 100%
// CPU on the three lines below. It is rejected in milliseconds now.
trait X extends Y with Z
trait Y extends Z
trait Z extends X

// The same shape without the branching, over classes rather than traits.
class C extends D
class D extends C

// A trait that is its own parent.
trait S extends S
