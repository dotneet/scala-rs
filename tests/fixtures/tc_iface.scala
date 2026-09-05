// nsc 2.13's trait ABI: concrete members are `default` methods on the
// interface, each with a `public static m$($this, ...)` beside it, and
// `$init$` is a `static` method on the interface too. No `T$class` anywhere.
//
// One fixture, several shapes: a concrete method, one that calls an abstract
// one, a `val` set from `$init$`, a `var` with a plain setter, a `private`
// helper (which stays a `private static` with no declaration and no
// forwarder), a lambda in a trait body (hoisted onto the interface), a
// stackable `abstract override`, an `object` mixing a trait in, and a trait
// method reached through `super` from a class.

trait Greet {
  val greeting: String = "hello"
  var seen: Int = 0
  private def punct: String = "!"
  def name: String
  def greet: String = greeting + ", " + name + punct
  def counted: String = { seen = seen + 1; greet + seen }
  def mapped: String = {
    val f: Int => Int = _ + name.length
    f(1) + "," + f(2) + "," + f(3)
  }
}

trait Base { def tag: String = "base" }
trait Mid extends Base { abstract override def tag: String = "mid(" + super.tag + ")" }
trait Top extends Base { abstract override def tag: String = "top(" + super.tag + ")" }

class Person(val name: String) extends Greet

class Louder extends Greet {
  def name = "sub"
  override def greet: String = "<" + super.greet + ">"
}

class Stack extends Base with Mid with Top

object Single extends Greet {
  def name = "object"
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Person("world")
    println(p.greet)
    println(p.counted)
    println(p.counted)
    println(p.mapped)
    println(p.greeting)
    println(new Louder().greet)
    println(new Stack().tag)
    println(Single.greet)
    val g: Greet = p
    println(g.name + "/" + g.seen)
  }
}
