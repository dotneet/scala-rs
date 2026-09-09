import scala.collection.immutable._
case class Z[A](value: ArraySeq[A])
object Main {
  def warm(a: scala.collection.immutable.ArraySeq[Int]) = a.sorted
  def zip(a: Seq[Int], b: Seq[Int]) = a.lazyZip(b)
  def ap[A, B](ff: Z[A => B], fa: Z[A])=
    Z(ff.value.lazyZip(fa.value).map(_.apply(_)))
  def main(args: Array[String]): Unit = {
    println(ap(Z(ArraySeq.unsafeWrapArray(Array((i: Int) => i + 1))), Z(ArraySeq.unsafeWrapArray(Array(4)))).value.head)
  }
}
