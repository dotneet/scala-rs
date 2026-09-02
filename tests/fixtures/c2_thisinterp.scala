// `$this` in a string interpolation.
//
// `this` is a keyword, not an identifier, so the `$`-form of an interpolation
// has to read it as the expression it spells -- `s"for $this"` means `this`,
// exactly like `s"for ${this}"`. Read as an identifier it was looked up as a
// term and failed with "not found: value this"; slick's
// `s"No type for symbol $sym found in $this"` is written this way.

class Node(val name: String) {
  override def toString: String = "Node(" + name + ")"
  def describe(n: Int): String = s"$n of $this"
  def braced(n: Int): String = s"$n of ${this}"
  // Inside a lambda the interpolation still means the enclosing instance,
  // not the lambda's own class.
  def all: String = {
    val f: Int => String = i => s"$i@$this"
    f(1) + "," + f(2)
  }
}

trait Tagged {
  def tag: String
  // A trait's `this` is the instance the trait is mixed into.
  def show: String = s"tagged $this"
}

class Leaf extends Node("leaf") with Tagged {
  def tag: String = "leaf"
}

object Main {
  override def toString: String = "MAIN"
  def here(n: Int): String = s"$n in $this"

  def main(args: Array[String]): Unit = {
    val n = new Node("a")
    println(n.describe(2))
    println(n.braced(3))
    println(n.all)
    val l = new Leaf
    println(l.show)
    println(l.describe(4))
    println(here(5))
  }
}
