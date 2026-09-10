abstract class A { val data: List[A] }
trait B[T <: B[T]] extends A { self: T => }
abstract class C extends A { val data: List[C] }
abstract class D extends C with B[D]
object OnlyA extends A { val data = Nil }
object Main extends D { val data = List(OnlyA) }
