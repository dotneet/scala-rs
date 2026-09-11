// toString of library values and of primitives through every path: string
// concatenation, interpolation, mkString, println(Any), and String.valueOf.
object Main {
  class Plain
  case class CC(a: Int, b: List[String], c: Option[Double])
  sealed trait T; case object Obj extends T; case class Leaf(v: Char) extends T
  def main(args: Array[String]): Unit = {
    println(List(1, 2)); println(Vector("a")); println(Set(1)); println(Map(1 -> "one")); println(Seq(1.5)); println(1 to 3); println(1 until 10 by 3)
    println(Some(1)); println(None); println(Left("l")); println(Right(2.0)); println((1, 'c', "s", 2L, 3.0f, true, ()))
    println(Nil); println(List()); println(List(List(1), Nil)); println(Array(1, 2).toList); println(Array("x").toSeq)
    println(CC(1, List("x"), Some(2.5))); println(Obj); println(Leaf('q')); println(List[T](Obj, Leaf('z')))
    println(()); println(1.0); println(1.0f); println(1e100); println(-0.0f); println(Long.MaxValue); println('x'); println(0.1 + 0.2); println(100.0f / 3)
    println(BigInt(123) + " " + BigDecimal("1.50") + " " + BigDecimal(0.1) + " " + scala.util.Try(1) + " " + scala.util.Success("x"))
    println(scala.collection.mutable.ArrayBuffer(1, 2) + " " + scala.collection.mutable.ListBuffer("a") + " " + scala.collection.mutable.Map(1 -> 2) + " " + scala.collection.mutable.Set(3))
    println(new StringBuilder("sb") + " " + "".isEmpty + " " + List('a', 'b') + " " + List(1.0f) + " " + List(1L) + " " + List(true))
    println(Iterator(1).toString.nonEmpty + " " + LazyList(1, 2).toString + " " + (LazyList(1, 2).force).toString)
    println(Some(List(Map(1 -> Set(2)))) + " " + (1 -> (2 -> 3)) + " " + Array(1, 2).mkString("[", ";", "]"))
    println(String.valueOf(1) + String.valueOf('c') + String.valueOf(true) + String.valueOf(2.5) + String.valueOf(3L))
    println(Int.box(1).toString + " " + Double.box(1.0) + " " + Character.valueOf('z') + " " + java.lang.Boolean.TRUE)
    println(s"${()} ${1.0f} ${'c'} ${List()} ${Option.empty[Int]}")
    println((new Plain).toString.startsWith("Main$Plain@"))
    println(Obj.getClass.getName + " " + Leaf('a').getClass.getName + " " + CC.getClass.getName + " " + Main.getClass.getName)
    println(classOf[CC].getName + " " + classOf[Int] + " " + classOf[Array[String]].getName + " " + classOf[List[_]].getName + " " + classOf[Unit])
    println((1: Any).getClass + " " + (1.0: Any).getClass + " " + ('c': Any).getClass + " " + ((): Any).getClass + " " + (true: Any).getClass)
    println(List(1, 2).getClass.getName + " " + Nil.getClass.getName + " " + Vector().getClass.getName + " " + Map().getClass.getName + " " + Map(1 -> 1).getClass.getName)
    println("x".getClass.getName + " " + (() => 1).getClass.getName.contains("Lambda") + " " + Some(1).getClass.getSimpleName)
    println(Double.PositiveInfinity + " " + Float.NaN + " " + Double.MinPositiveValue + " " + Float.MaxValue + " " + Short.MinValue + " " + Byte.MaxValue + " " + Char.MaxValue.toInt)
  }
}
