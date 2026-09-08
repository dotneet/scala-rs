// A constructor's written type arguments were never checked against the
// class's declared bounds. `new Bounded2[Int, String]` for
// `class Bounded2[K <: AnyRef, V]` compiled, and the class file it produced
// names a type the program is not allowed to build.
//
// nsc reports this in refchecks (`checkBounds`), so this file must contain no
// typer error of its own: with one, real scalac never reaches refchecks and
// there would be nothing to compare against. It therefore holds the bounds
// violation and nothing else -- the under-applied type-argument list lives in
// `ovsc_targcount_bad.scala` for the same reason.
//
// scalac 2.13.16 reports it twice, once for the `val`'s inferred type and once
// for the `new` itself, both on the same line; this compiler reports the `new`.
package ovsc

class Bounded2[K <: AnyRef, V](val k: K, val v: V)

object BoundsBad {
  def main(args: Array[String]): Unit = {
    val a = new Bounded2[Int, String](0, "x")
    println(a.k)
  }
}
