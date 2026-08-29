// Regressions for the second `type mismatch` slice: type arguments that were
// dropped or never solved, and members whose real signature never arrived.
//
// Every definition here is accepted by scalac 2.13.16; the expected output is
// what nsc prints for the same program.

import scala.collection.mutable
import scala.reflect.{classTag, ClassTag}

// -- a default argument may name a member declared further down -------------
object Uses {
  def greet(who: String = Later.name): String = "hi " + who
}
object Later {
  val name: String = "later"
}

// -- `map` on a type with more than one type parameter ----------------------
trait NoStream
trait Effect
trait Act[+R, +S <: NoStream, -E <: Effect] {
  def value: R
  def map[R2](f: R => R2): Act[R2, NoStream, E] = Act.pure[R2, E](f(value))
}
object Act {
  def pure[R, E <: Effect](r: R): Act[R, NoStream, E] =
    new Act[R, NoStream, E] { def value: R = r }
  def seq[R, E <: Effect](rs: Seq[R]): Act[Seq[R], NoStream, E] = pure[Seq[R], E](rs)
  // `R2` is only determined by the lambda's *result*, and `R` is covariant, so
  // the expected type says nothing about it.
  def headOf[R, E <: Effect](rs: Seq[R]): Act[Option[R], NoStream, E] =
    seq[R, E](rs).map(_.headOption)
}

// -- `Obj[T1, T2, T3]` with an implicit, parameterless `apply` --------------
trait Shape[L, M, U, P] {
  def name: String
}
class Rep[T](val t: T)
object RepShape {
  def apply[L, M, U]: Shape[L, M, U, M] = new Shape[L, M, U, M] {
    def name: String = "rep"
  }
}
object Shapes {
  def repShape[L, T]: Shape[L, Rep[T], T, Rep[T]] = RepShape[L, Rep[T], T]
}

// -- an argument's own type arguments come from the parameter it fills ------
class Box(val m: Map[String, Int])

object Main {
  // A local `def` is in scope for the whole block, so `first` may call
  // `second` before it is written.
  def localDefs(n: Int): Int = {
    def first(x: Int): (String, Int) = second(x)
    def second(x: Int): (String, Int) = ("ab", x)
    val (s, i) = first(n)
    s.length + i
  }

  def main(args: Array[String]): Unit = {
    println(Uses.greet())
    println(Act.headOf[Int, Effect](Seq(7, 8, 9)).value)
    println(Shapes.repShape[String, Int].name)

    // `Coll.empty` takes its type arguments from the expected type.
    val v: Vector[Int] = Vector.empty
    val m: mutable.HashMap[String, Int] = mutable.HashMap.empty
    val i: Iterable[(String, Int)] = Vector.empty
    val boxed = new Box(Map.empty)
    println(v.size + m.size + i.size + boxed.m.size)

    // Two local `def`s may call each other, whichever comes first.
    println(localDefs(3))

    // A tuple component that is a function literal gets its parameter type
    // from the expected type.
    val p: (Int, Int => Int) = (1, n => n + 1)
    val fs: List[Int => Int] = List(n => n * 2)
    val doubler: Int => Int = fs.head
    println(p._2(p._1) + doubler(20))

    // `scala.reflect.classTag`'s implicit clause survives the classfile.
    val ct: ClassTag[_] = classTag[Short]
    println(ct.runtimeClass.getName)
  }
}
