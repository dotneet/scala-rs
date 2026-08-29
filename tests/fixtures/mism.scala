import scala.annotation.unchecked.uncheckedVariance

// An invariant class: nothing hides a type argument that was widened to `Any`.
class Inv[T](val value: T) {
  def self: Inv[T] = this
}

// A contravariant parameter, so joining two of these has to go the other way.
trait Eff
trait EA extends Eff
trait EB extends Eff
trait NoStream
trait Act[+R, +S <: NoStream, -E <: Eff] {
  def name: String
  def label: String = "act:" + name
}

class Leaf[+R, +S <: NoStream, -E <: Eff](val name: String) extends Act[R, S, E]

class Sub[A](n: String) extends Leaf[A, NoStream, Eff](n) {
  override def label: String = "sub(" + super.label + ")"
}

object Mism {
  // A method type parameter is inferred from an argument whose type mentions
  // the *caller's* type parameter: `A` is `Inv[T]`, not `Inv[Any]`.
  def firstOf[A](a: A, b: A): A = a
  def pairUp[T](a: Inv[T], b: Inv[T]): Inv[T] = firstOf(a, b)
  def tupled[T](a: Inv[T], b: Inv[T]): (Inv[T], Inv[T]) = (a, b)

  // `this` in a generic class carries the class's own type arguments.
  def viaThis[T](a: Inv[T]): Inv[T] = a.self

  // The join of two `Act`s keeps `E` on the contravariant side.
  def joined[E1 <: Eff, E2 <: Eff](
    x: Act[Int, NoStream, E1],
    y: Act[String, NoStream, E2]
  ): Vector[Act[Any, NoStream, E1 with E2]] = Vector(x, y)

  // The collection hierarchy: a `Vector` really is a `Seq` and an `Iterable`.
  def asSeq(v: Vector[Int]): Seq[Int] = v
  def asIterable(v: Vector[Int]): Iterable[Int] = v
  def setAsIterable(s: Set[Int]): Iterable[Int] = s
  def mapAsIterable(m: Map[Int, String]): Iterable[(Int, String)] = m
  def nilAsSeq: Seq[String] = Nil

  // A module's singleton type is the module.
  def someNil: Some[Nil.type] = Some(Nil)

  // Annotations do not stand in the way of conformance.
  def annotated(f: Inv[Int] => (Inv[Int] @uncheckedVariance)): Int = f(new Inv(1)).value

  // A polymorphic method eta-expanded against an expected function type has
  // its own type parameters solved first.
  def useFn(f: Inv[Int] => Inv[Int]): Inv[Int] = f(new Inv(7))
  val ident: Inv[Int] => Inv[Int] = identity
}

object Main {
  def main(args: Array[String]): Unit = {
    val a = new Inv(1)
    val b = new Inv(2)
    println(Mism.pairUp(a, b).value)
    println(Mism.tupled(a, b)._2.value)
    println(Mism.viaThis(a).value)
    println(Mism.joined(new Leaf[Int, NoStream, EA]("x"), new Leaf[String, NoStream, EB]("y")).length)
    println(Mism.asSeq(Vector(1, 2, 3)).mkString(","))
    println(Mism.asIterable(Vector(4, 5)).mkString(","))
    println(Mism.setAsIterable(Set(9)).mkString(","))
    println(Mism.mapAsIterable(Map(1 -> "one")).mkString(","))
    println(Mism.nilAsSeq.length)
    println(Mism.someNil.get.length)
    println(Mism.annotated(x => x))
    println(Mism.useFn(Mism.ident).value)
    println(new Sub[Int]("s").label)
  }
}
