// The fifth slice must not turn its relaxations into silence. scalac 2.13.16
// rejects every definition below (the wording differs).

// A trait that extends a function type is a SAM only while `apply` is its one
// abstract method. A second one leaves nothing for a function literal to be.
trait TwoAbs6 extends (Int => String) {
  def other: Int
}
object Sam6 {
  // nsc: missing parameter type / TwoAbs6 is not a functional interface
  val bad: TwoAbs6 = i => i.toString
}

// The type arguments an `extends` clause leaves out are *inferred*, not
// invented: an argument that fits no instantiation is still an error.
class Seqn6[T](val v: T)
class Base6[T](val s: Seqn6[T])
class Derived6[T](s2: Seqn6[T], other: String) extends Base6(other)

// A `new` reads its type arguments off the expected type; where the expected
// type is not a base class instance at all, nothing is read and the error
// stands.
trait RC6[R, U]
class UnitRC6[R] extends RC6[R, Unit]
object New6 {
  // nsc: type mismatch; found: UnitRC6[String]; required: RC6[String,Int]
  def bad6: RC6[String, Int] = new UnitRC6[String]
}

// A same-element-type transformation keeps the *receiver's* collection, which
// is not a licence to narrow: a `Seq` does not become a `Vector` by being
// filtered.
object Coll6 {
  val xs: Seq[Int] = Seq(1, 2, 3)
  // nsc: type mismatch; found: Seq[Int]; required: Vector[Int]
  val bad: Vector[Int] = xs.filter(_ > 1)
}

// The expected type widens a factory's element type only where the arguments
// really conform. `Set[String]` cannot hold an `Int`.
object Fac6 {
  // nsc: type mismatch; found: Int(1); required: String
  val bad: Set[String] = Set(1)
}

// Looking through an annotation for `.apply` does not invent one: a type with
// no `apply` member is still not a function.
class NoApply6
object Annot6 {
  val v: NoApply6 @unchecked = new NoApply6
  // nsc: NoApply6 does not take parameters
  val bad: Int = v(1)
}
