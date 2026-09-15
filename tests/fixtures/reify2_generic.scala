import scala.reflect.runtime.universe._
import scala.tools.reflect.Eval

class ReifiedHolder[A](val value: A) {
  def expr: Expr[A] = reify { value }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new ReifiedHolder[Int](42).expr.eval)
    println(new ReifiedHolder[String]("ok").expr.eval)
  }
}
