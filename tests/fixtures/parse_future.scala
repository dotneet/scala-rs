// Compiled with -Xsource:3: the soft modifiers `open` / `infix`, the `-_` /
// `+_` type names, and `xs*` splices and patterns.

open class A { def id = "A" }
infix class B[T, S]
open infix case class CC[T, S](x: Int)
infix trait DT[T, S]

open
infix
private
class E

class F {
  open infix class C1[T, S]
  infix type X = Int
  infix def foo(x: Int): Int = x + 1
}

object Main {
  type `-_` = Int
  type `+_` = Long

  val fn: -_ => +_ = (_: Int).toLong
  val fn2: (-_) => +_ = fn
  val opt: Option[ + _ ] = Some[ + _ ](1L)

  def foo(xs: Int*): Seq[Int] = xs

  def main(args: Array[String]): Unit = {
    val infix: Int = 1
    println(infix + 1)
    val open: Int => Int = x => x * 10
    println(open(1))
    println(new A().id + " " + new F().foo(1) + " " + CC[Int, Int](3).x)
    println(fn(2) + fn2(3) + opt.get)
    val s: Seq[Int] = Seq(1, 2, 3)
    println(foo(s*))
    println(foo((s ++ s)*))
    s match {
      case Seq(x, rest*) => println(s"$x $rest")
    }
  }
}
