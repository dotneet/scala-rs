// `IterableFactory` members whose `CC` is bounded by one of the library's
// `…Ops` traits (`List$.fill` is `(ILscala/Function0;)Lscala/collection/SeqOps;`
// on the JVM, exactly as in the real jar). The result has to be cast back to
// the collection the typer settled on before anything uses it, or the verifier
// rejects the method.
import scala.collection.mutable.{ArrayBuffer, ListBuffer}

object Main {
  def main(args: Array[String]): Unit = {
    // Argument of `:::` (which is right-associative, so the factory result is
    // the *argument*, not the receiver).
    println(List.fill(2)(5) ::: List(9))
    println(List.tabulate(3)(i => i) ::: List(9))
    println(List.concat(List(1), List(2)) ::: List(9))
    println(List.iterate(1, 3)(_ + 1) ::: List(9))
    println(List.empty[Int] ::: List(9))
    println(List.unfold(0)(s => if (s < 3) Some((s, s + 1)) else None) ::: List(9))

    // Receiver position.
    println(List.fill(2)(5).head)
    println(List.fill(2)(5).reverse)
    println(List.fill(2)(5).length)
    println(List.tabulate(3)(i => i).head)
    println(List.concat(List(1), List(2)).last)
    println(Vector.fill(2)(5).length)
    println(Vector.tabulate(3)(i => i).head)
    println(Seq.fill(2)(5).head)
    println(Set.fill(2)(5).size)
    println(ArrayBuffer.fill(2)(5).size)
    println(ListBuffer.fill(2)(5).size)

    // Through a `val`, through an ascription, and as a `match` scrutinee.
    val xs = List.fill(2)(5)
    println(xs ::: List(9))
    println((List.fill(2)(5): List[Int]) ::: List(9))
    println(List.fill(2)(5) match {
      case h :: _ => h
      case Nil    => -1
    })

    // Chained: the cast has to happen on each step, not only the last.
    println(List.fill(2)(5).reverse ::: List(9))
    println(List.tabulate(3)(i => i).map(_ + 1) ::: List(9))
  }
}
