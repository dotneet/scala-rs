case class Multi(a: Int)(val b: String)(implicit ev: Ordering[Int])
case class C[A: Ordering](a: A)(implicit n: Numeric[A])

object Main {
  def main(args: Array[String]): Unit = {
    println(ConstructorReflection.inspect[Multi])
    println(ConstructorReflection.inspect[C[Int]])
    val multi = Multi(1)("body")(Ordering.Int)
    val generic = C[Int](1)(Ordering.Int, implicitly[Numeric[Int]])
    println(s"${multi.a}:${multi.b}:${generic.a}")
    println(generic.copy(a = 2).a)
  }
}
