// A narrower (covariant) result type in an override needs a bridge with the
// parent's erased signature. A `case object` used to miss it: the bridge
// emitter only ran for classes, so `Direction.reverse` stayed abstract and
// any call through the parent type threw `AbstractMethodError`.
sealed abstract class Direction {
  def reverse: Direction
  def name: String
}
case object Asc extends Direction {
  override def reverse: Desc.type = Desc
  def name: String = "asc"
}
case object Desc extends Direction {
  override def reverse: Asc.type = Asc
  def name: String = "desc"
}

abstract class Animal {
  def self: Animal
  def tag: String
}
class Dog extends Animal {
  override def self: Dog = this
  def tag: String = "dog"
}

// The same thing through a trait, and with a nested class as the result type.
trait Box {
  def unwrap: AnyRef
}
object Wrapper extends Box {
  override def unwrap: String = "wrapped"
}

object Main {
  def main(args: Array[String]): Unit = {
    val d: Direction = Asc
    println(d.reverse.name)
    println(d.reverse.reverse.name)
    val a: Animal = new Dog
    println(a.self.tag)
    val b: Box = Wrapper
    println(b.unwrap)
  }
}
