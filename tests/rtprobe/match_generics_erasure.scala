// Typed patterns against generic types (erased: List[String] matches any
// List), arrays (not erased), and primitive/boxed interplay in patterns.
object Main {
  def erased(x: Any): String = x match {
    case _: List[String @unchecked] => "a list"
    case _: Map[_, _] => "a map"
    case _: Option[Int @unchecked] => "an option"
    case _ => "other"
  }
  def arrays(x: Any): String = x match {
    case a: Array[Int] => "ints " + a.sum
    case a: Array[Double] => "doubles " + a.sum
    case a: Array[String] => "strings " + a.mkString
    case a: Array[AnyRef] => "refs " + a.length
    case _ => "not array"
  }
  def gen[T](x: Any, witness: T): String = x match {
    case t: T @unchecked => "matched T (unchecked)"
  }
  def boxes(x: Any): String = x match {
    case i: java.lang.Integer => "jInteger " + (i + 1)
    case l: java.lang.Long => "jLong " + (l + 1)
    case c: java.lang.Character => "jChar " + c
    case n: Number => "number " + n
    case _ => "?"
  }
  def firstInt(xs: List[Any]): Int = xs match {
    case (h: Int) :: _ => h
    case _ :: t => firstInt(t)
    case Nil => -1
  }
  sealed trait Res[+A]
  case class Ok[A](a: A) extends Res[A]
  case class Err(msg: String) extends Res[Nothing]
  def res[A](r: Res[A]): String = r match {
    case Ok(n: Int) => "ok int " + (n * 2)
    case Ok(s: String) => "ok string " + s.length
    case Ok(other) => "ok " + other
    case Err(m) => "err " + m
  }

  def main(args: Array[String]): Unit = {
    println(erased(List(1, 2))); println(erased(Map(1 -> 2))); println(erased(Some("x"))); println(erased(None))
    println(arrays(Array(1, 2, 3))); println(arrays(Array(1.5, 2.5))); println(arrays(Array("a", "b")))
    println(arrays(Array(List(1)))); println(arrays(List(1)))
    println(gen("s", 1))
    println(boxes(3)); println(boxes(4L)); println(boxes('q')); println(boxes(2.5)); println(boxes(BigInt(9)))
    println(firstInt(List("a", 'b', 5, 6))); println(firstInt(List("x")))
    println(res(Ok(21))); println(res(Ok("four"))); println(res(Ok(1.5))); println(res(Err("bad")))
  }
}
