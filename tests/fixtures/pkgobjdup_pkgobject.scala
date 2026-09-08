// One name supplied twice: by a package and by that package's package object.
//
// A package object's members *are* its package's members (SLS 9.3), so two
// entries under one name are not an overload set -- they are one name reached
// by two routes that stand in no `extends` relation, which no ordering rule
// can reduce. nsc's `openPackageModule` unlinks whatever the package already
// had under a name the package object declares, in that name's own namespace,
// and only then enters the package object's members.
//
// Compiling scala/scala's `src/library`, this is `scala/package.scala`'s
// `val Nil = scala.collection.immutable.Nil` beside the prelude's own idea of
// `scala.Nil`, and every use of `Nil` printed `<overload Nil$ | Nil$>` -- the
// same type twice, which is what a duplicate *supply* looks like.
//
// On the pre-fix binary this file does not compile:
//   error: value eq is not a member of <overload Payload$ | Payload$>
//   error: type mismatch; found: Box  required: Box
// which are the term half and the type half of the same defect. Every line
// below is checked by its value against real scalac 2.13.16.

package inner {
  object Payload { override def toString = "inner.Payload" }
  class Box(val n: Int) { override def toString = "Box(" + n + ")" }
}

package outer {
  // Supplied by the *package*.
  object Payload { def tag: String = "outer's own object Payload" }
  class Box(val s: String) { override def toString = "outer.Box(" + s + ")" }
  // `Thing` is only ever shadowed in the *type* namespace below, so this
  // object has to stay reachable in term position: the two namespaces are
  // separate and unlinking one must not touch the other.
  object Thing { def tag: String = "outer's own object Thing" }
}

package object outer {
  // Supplied by the *package object*, and by nsc's rule these win.
  val Payload = inner.Payload
  type Box = inner.Box
  type Thing = inner.Box
  // The package object's own overload set has to survive the unlink pass.
  // Unlinking member by member instead of in a pass of its own has these two
  // remove each other: `scala.math`'s package object is full of them, and the
  // library went from 836 errors to 924 that way.
  def pick(n: Int): String = "pick(Int)"
  def pick(d: Double): String = "pick(Double)"
}

object Main {
  def main(args: Array[String]): Unit = {
    // The entity reached by both routes, shown to be one value.
    println(outer.Payload)
    println(outer.Payload eq inner.Payload)
    // The type half: `outer.Box` is the package object's alias, not the
    // package's own class, so an `inner.Box` conforms to it.
    val b: outer.Box = new inner.Box(7)
    println(b)
    println(outer.pick(1))
    println(outer.pick(1.5))
    // Type namespace shadowed, term namespace not.
    val t: outer.Thing = new inner.Box(9)
    println(t)
    println(outer.Thing.tag)
    // The standard library's own instance of the shape: `scala.Nil` and
    // `scala.List` *are* the `scala` package object's `val`s, and a program
    // that reaches them unqualified and qualified must get one value.
    println(Nil eq scala.Nil)
    println(List eq scala.List)
    println((Nil: List[Int]).length)
  }
}
