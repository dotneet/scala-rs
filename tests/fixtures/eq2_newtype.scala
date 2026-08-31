// `new` still works for real class types, type constructors applied to a
// type parameter, and type *aliases* (`type Self = ConcreteNamed`) -- only a
// bare type parameter or abstract type member is rejected.
class Box[T](val value: T) {
  def dup: Box[T] = new Box[T](value)
}
trait Named {
  type Self
  def self: Self
}
class ConcreteNamed extends Named {
  type Self = ConcreteNamed
  def self: Self = new Self
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Box[Int](5).dup.value)
    println(new ConcreteNamed().self.isInstanceOf[ConcreteNamed])
  }
}
