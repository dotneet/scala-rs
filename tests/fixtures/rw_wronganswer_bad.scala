// The other half of "a `private` member of a trait is not inherited": once
// the member traversal stops answering with it, a subclass that names it has
// to be *rejected*, not silently bound to the parent's private field.
// scalac: `not found: value hidden`.

trait Secretive { private val hidden = 1 }

class Peeker extends Secretive {
  def peek: Int = hidden
}

object Main {
  def main(args: Array[String]): Unit = println(new Peeker().peek)
}
