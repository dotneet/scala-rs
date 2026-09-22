object Main {
  final class Holder { var value: String = _ }

  def upcast[A, B](x: A)(implicit ev: A <:< B): B = ev(x)
  def sameType[A, B](x: A)(implicit ev: A =:= B): B = ev(x)

  def sumAll(xs: Iterable[Int]): Int = {
    var total = 0
    xs.foreach(x => total += x)
    total
  }

  def main(args: Array[String]): Unit = {
    val n: Any = upcast[Int, Any](42)
    println(n)
    val s: String = sameType[String, String]("hello")
    println(s)
    val some: Option[String] = Some("present")
    println(some.orNull)
    val none: Option[String] = None
    println(none.orNull)
    // `orNull`'s result type may widen past its element type. Generated
    // field-number matches use `Option[Int].orNull` as `Any`.
    val noInt: Any = Option.empty[Int].orNull
    val someInt: Any = Option(7).orNull
    println(noInt)
    println(someInt)
    // `=:=[A, A]` is the implicit witness for a `<:<[Null, String]`
    // request. Inference has to read `=:=` at its `<:<` base before solving A.
    val inferred: String = some.orNull
    val holder = new Holder
    holder.value = none.orNull
    println(sumAll(List(1, 2, 3, 4)))
  }
}
