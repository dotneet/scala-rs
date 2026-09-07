// Consumed by scala-rs against `Lib_1.scala`'s class files alone.
//
// `class Sub extends Leafy(...)` is what forces `Profile#API` to be adopted:
// a parent list is resolved before the bodies are typed, so the adoption
// happens between the first walk of the wildcard import and the later ones.
// Without the `pickle_readable` guard in `Typer::import_wildcard`, the first
// walk memoizes a refusal for `toRich` and `toNamed`, that adoption gets the
// memo instead of the pickled signatures, and both conversions sit in the
// implicit scope as plain class-file methods that can never be selected:
// "value described is not a member of 3".
//
// Every conversion is called and printed, so a wrong one cannot pass as green.
import iglib.TheProfile.api._

class Sub(label: String) extends Leafy(label)

object Main {
  def main(args: Array[String]): Unit = {
    println(new Sub("x").label)
    println(3.described)
    println(3.naming)
    println(toPlain(3).plainly)
  }
}
