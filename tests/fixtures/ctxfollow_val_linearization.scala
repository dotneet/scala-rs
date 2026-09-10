abstract class A { val data: List[A] }
trait B[T <: B[T]] extends A { self: T => }
abstract class C extends A { val data: List[C] }
abstract class D extends C with B[D]
object N extends D { val data = Nil }
object M extends D { val data = List(N) }
object Main { def main(args: Array[String]): Unit = {
  val cs: List[C] = M.data
  println(cs.size)
  println(cs.head eq N)
}}
