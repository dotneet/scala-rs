// Pattern-match warnings nsc's patmat phase issues by default: exhaustivity
// with counter-examples, unreachable cases, variable patterns, switches.
import scala.annotation.switch

sealed trait Shape
case object Dot extends Shape
case class Circle(r: Int) extends Shape
case class Rect(w: Int, h: Int) extends Shape

object WarnPatmat {
  def area(s: Shape): Int = s match {
    case Dot => 0
    case Circle(r) => r * r
  }

  def opt(o: Option[Int]): Int = o match {
    case Some(x) if x > 1 => x
    case None => 0
  }

  def list(l: List[Int]): Int = l match {
    case x :: Nil => x
    case Nil => 0
  }

  def pair(p: (Boolean, Boolean)): Int = p match {
    case (true, true) | (false, false) => 1
  }

  def dup(i: Int): String = i match {
    case 1 => "one"
    case 2 => "two"
    case 1 => "uno"
    case _ => "many"
  }

  def typed(a: Any): Int = a match {
    case _: String => 1
    case _: String => 2
    case _ => 3
  }

  def variable(s: String): Int = s match {
    case "a" => 1
    case other => 2
    case "b" => 3
  }

  def alts(i: Int): Int = (i: @switch) match {
    case 0 | 0 => 0
    case 2 | 2 | 3 | 3 => 1
    case _ => 2
  }

  def unchecked(s: Shape): Int = (s: @unchecked) match {
    case Dot => 0
  }

  def main(args: Array[String]): Unit = {
    println(area(Circle(2)))
    println(opt(Some(3)))
    println(list(List(4)))
    println(pair((true, true)))
    println(dup(1))
    println(typed("s"))
    println(variable("x"))
    println(alts(3))
    println(unchecked(Dot))
  }
}
