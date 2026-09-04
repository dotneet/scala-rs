// agent/cats: four things typelevel/cats writes that this compiler did not
// accept. All four are plain Scala 2.13 -- no compiler plugin involved.
//
// 1. `$` is a letter in an identifier (nsc `Chars.isIdentifierStart`). cats'
//    checked-in simulacrum output writes `implicit ev$1: Defer[G]`.
// 2. A type parameter may carry annotations (`TypeParam ::= {Annotation} ...`).
//    cats-kernel writes `trait Eq[@sp A]` on 26 traits.
// 3. `@tailrec` on a def *nested in a method* is fine: it is not a member of
//    anything, so nothing can override it. cats writes it 79 times inside
//    `tailRecM`.
// 4. A package written out in an expression reaches its package object:
//    `cats.kernel.instances.int.catsKernelStdOrderForInt`.

import scala.annotation.tailrec

class marker extends scala.annotation.StaticAnnotation

trait Adder[@marker A] {
  def combine(x: A, y: A): A
}

package pkgobj {
  trait Base { val base: Int = 40 }
  package object inner extends Base
}

object Main {
  def bump(ev$1: Int): Int = {
    val x$y = ev$1 + 1
    x$y
  }

  def total(n: Int): Int = {
    @tailrec
    def loop(i: Int, acc: Int): Int =
      if (i == 0) acc else loop(i - 1, acc + i)
    loop(n, 0)
  }

  val ints: Adder[Int] = new Adder[Int] {
    def combine(x: Int, y: Int): Int = x + y
  }

  def main(args: Array[String]): Unit = {
    println(bump(1))
    println(total(4))
    println(ints.combine(3, 4))
    println(pkgobj.inner.base)
  }
}
