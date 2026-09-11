// A lambda body typed under an undetermined result (nsc's `typedFunction`
// with `WildcardType`): an `if` / `match` of numeric branches takes the
// weak-conformance lub (`Long`), not `AnyVal` with both branches boxed --
// `List(1, 2).map(x => if (x > 1) 1L else 0)` is a `List[Long]`, and its
// element classes say so at run time.
object Main {
  def ap[A, B](a: A)(f: A => B): B = f(a)
  def two[A, B](a: A, f: A => B): B = f(a)
  class Bx[A](val a: A)
  def main(args: Array[String]): Unit = {
    val l1 = List(1, 2).map(x => if (x > 1) 1L else 0)
    println(l1.map(_.getClass.getSimpleName))
    val l2 = List(1, 2).map(x => x match { case 1 => 1.0; case _ => 2 })
    println(l2.map(_.getClass.getSimpleName))
    val l3 = List(1, 2).map { x => if (x > 1) 'a' else 0 }
    println(l3.map(_.getClass.getSimpleName))
    val r = ap(3)(x => if (x > 1) 1L else 0)
    println(r.getClass.getSimpleName)
    val l4 = List(1, 2).map { x => val y = x * 2; if (y > 2) 1.5f else 3 }
    println(l4.map(_.getClass.getSimpleName))
    // Declared result: no numeric lub, `0` stays an Int boxed to AnyVal.
    val l5: List[AnyVal] = List(1, 2).map(x => if (x > 1) 1L else 0)
    println(l5.map(_.getClass.getSimpleName))
    val f6: Int => AnyVal = x => if (x > 1) 1L else 0
    println(List(1, 2).map(f6).map(_.getClass.getSimpleName))
    // Non-numeric branches stay a real lub.
    println(List(1, 2).map(x => if (x > 1) "s" else 0).map(_.getClass.getSimpleName))
    // Nested lambdas.
    println(List(1, 2).map(x => List(x).map(y => if (y > 1) 1L else 0)).map(_.map(_.getClass.getSimpleName)))
    // A polymorphic reference as the body is still solved by the
    // declaration or minimised, never left as a stray parameter.
    val e1: List[List[Int]] = List(1, 2).map(x => List.empty)
    val e2: List[Option[String]] = List(1, 2).map(x => None)
    val e3: List[Int] = List[Int]().map(x => ???)
    println((e1, e2, e3))
    println(List(1, 2).map(x => Map.empty[String, Int] + ("a" -> x)))
    println(List(1, 2).flatMap(x => if (x > 1) List(1L) else List(0)).map(_.getClass.getSimpleName))
    println(two(3, (x: Int) => if (x > 1) 1L else 0).getClass.getSimpleName)
    println(ap(3)(x => if (x > 1) 'a' else 0).getClass.getSimpleName)
    val b: Bx[Int] = ap(1)(x => new Bx(x))
    println(b.a)
    println(List(1, 2).map[Long](x => if (x > 1) 1L else 0).map(_.getClass.getSimpleName))
    println(List(1, 2).map(x => if (x > 1) "a" else "b"))
    println(List(1, 2).map(x => x > 1))
  }
}
