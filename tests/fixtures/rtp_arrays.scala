// A type test against `Array[_]` or `Array[T]` accepts every array --
// primitive ones included -- and nothing else (nsc `ScalaRunTime.isArray`).
// And a field `var v: T = _` of an abstract type defaults to `null`.
object Main {
  def f(x: Any): String = x match {
    case _: Array[Int] => "int array"
    case _: Array[_] => "some array"
    case _ => "other"
  }
  def g[T](x: Any): String = x match { case _: Array[T] @unchecked => "T array"; case _ => "no" }
  class Cell[T] { var v: T = _; def isSet = v != null }
  trait HasT { type T; var t: T = _ }
  class Str extends HasT { type T = String }
  def main(args: Array[String]): Unit = {
    println(List(Array(1), Array("s"), Array(1.5), List(1), "s").map(f))
    println(List(Array(1), Array("s"), List(1)).map(g[String]))
    println(Array(1.5).isInstanceOf[Array[_]] + " " + List(1).isInstanceOf[Array[_]] + " " + (null: Any).isInstanceOf[Array[_]])
    val c = new Cell[String]
    println(c.v + " " + c.isSet)
    c.v = "x"
    println(c.v + " " + c.isSet)
    val s = new Str
    println(s.t)
    s.t = "y"
    println(s.t)
  }
}
