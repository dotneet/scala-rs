// Compiled by real scalac into a directory, then mixed in by
// `gzero_traitmixin.scala`, which scala-rs compiles against the *class files*.
//
// Everything here is concrete, and none of it says so in the bytecode: the
// interface declares `greeting()` and `counter()` abstract because the value is
// assigned by `$init$` through `gzerolib$HasStuff$_setter_$greeting_$eq`, and
// it declares the nested objects' accessors abstract because the field and the
// lazy initialisation belong to the implementing class.
package gzerolib

trait HasStuff {
  val greeting: String = "hello"
  val counter: Int = 41
  var seen: Int = 0

  object Inner { override def toString = "inner" }
  object Other { override def toString = "other" }

  def show: String = s"$greeting/$counter/$Inner/$Other/$seen"
}

trait HasNested extends HasStuff {
  object Deeper { override def toString = "deeper" }
  def deep: String = s"$Deeper+$show"
}
