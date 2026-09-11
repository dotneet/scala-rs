// Pattern matching: literals, guards, typed patterns on Any (including
// primitives and boxed numbers), alternatives, `@` binders, fallthrough order.
object Main {
  def classify(x: Any): String = x match {
    case 0 => "zero"
    case i: Int if i < 0 => "negInt"
    case i: Int => "int" + i
    case 1L => "oneLong"
    case l: Long => "long" + l
    case 'c' => "charC"
    case ch: Char => "char" + ch
    case d: Double if d.isNaN => "nan"
    case d: Double => "double" + d
    case f: Float => "float" + f
    case b: Byte => "byte" + b
    case s: Short => "short" + s
    case true => "yes"
    case false => "no"
    case "" => "empty"
    case s: String if s.length > 3 => "long string"
    case s: String => "str:" + s
    case null => "null!"
    case () => "unit"
    case _: Array[Int] => "int array"
    case _: Array[_] => "some array"
    case _ => "other:" + x.getClass.getSimpleName
  }

  def alts(x: Any): String = x match {
    case 1 | 2 | 3 => "small"
    case "a" | "b" => "ab"
    case _: Int | _: Long => "integral"
    case Some(1 | 2) => "some-small"
    case _ => "?"
  }

  def binders(x: Any): String = x match {
    case s @ Some(v @ (1 | 2)) => s"$s has $v"
    case l @ List(a, b @ _*) => s"list $l head $a rest $b"
    case p @ (a: Int, _) => s"pair $p first $a"
    case other => "other " + other
  }

  def guards(n: Int): String = n match {
    case x if x % 15 == 0 => "FizzBuzz"
    case x if x % 3 == 0 => "Fizz"
    case x if x % 5 == 0 => "Buzz"
    case x => x.toString
  }

  def main(args: Array[String]): Unit = {
    val samples: List[Any] = List(0, -5, 7, 1L, 9L, 'c', 'z', Double.NaN, 2.5, 1.5f, 3.toByte, 4.toShort,
      true, false, "", "abcd", "ab", null, (), Array(1, 2), Array("x"), List(1), 1.0: Any)
    samples.foreach(s => println(classify(s)))
    List[Any](1, 3, "a", 5, 6L, Some(2), Some(9), 4.0).foreach(x => println(alts(x)))
    List[Any](Some(1), Some(3), List(1, 2, 3), List(9), (4, "x"), ("x", 4)).foreach(x => println(binders(x)))
    println((1 to 15).map(guards).mkString(" "))
  }
}
