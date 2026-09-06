final class Constant[A](val value: A) extends (Any => A) {
  def apply(a: Any): A = value
}
final class Wrapped[A, B](f: A => B) extends (A => B) {
  def apply(a: A): B = f(a)
}
final class Zero[A](val value: A) extends (() => A) {
  def apply(): A = value
}
object Main {
  def constant[A, B](f: A => B): String = f match {
    case c: Constant[B] => "constant"
    case _ => "other"
  }
  def wildcard[A, B](f: A => B): String = f match {
    case c: Constant[_] => "wildcard"
    case _ => "other"
  }
  def wrapped[A, B](f: A => B): String = f match {
    case c: Wrapped[A, B] @unchecked => "wrapped"
    case _ => "other"
  }
  def zero[A](f: () => A): String = f match {
    case c: Zero[A] @unchecked => "zero"
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(constant(new Constant(7)))
    println(wildcard(new Constant(7)))
    println(wrapped(new Wrapped[Int, Int](x => x + 1)))
    println(zero(new Zero(7)))
  }
}
