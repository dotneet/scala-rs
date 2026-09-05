// `import scala.{specialized => sp}` is how cats and the collections write it.
// The parser resolves the rename, so `@sp` is read as `@specialized` and not as
// an unknown user annotation. Library mode only: the private runtime has no
// `scala.specialized` / `scala.Specializable` for the imports to name.
import scala.{specialized => sp}
import scala.Specializable._

class Holder[@sp(Int, Long) T](val value: T) {
  def twice(f: T => T): T = f(f(value))
}

class Grouped[@sp(Primitives) A, @sp(Everything) B](val a: A, val b: B) {
  def show: String = a.toString + "/" + b.toString
}

object Main {
  def first[@sp(Bits32AndUp) A](xs: List[A]): A = xs.head

  def main(args: Array[String]): Unit = {
    println(new Holder(3).twice(_ * 2))
    println(new Grouped(1, "b").show)
    println(first(List(10, 20)))
  }
}
