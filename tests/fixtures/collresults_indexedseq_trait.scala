import scala.collection.immutable.{IndexedSeq => ImIndexedSeq}

class WrappedIndexedSeq[A](m: scala.collection.mutable.IndexedSeq[A]) extends ImIndexedSeq[A] {
  override def length: Int = m.length
  override def apply(i: Int): A = m(i)
  override def iterator: Iterator[A] = m.iterator
}

object Main {
  def main(args: Array[String]): Unit =
    println(new WrappedIndexedSeq(scala.collection.mutable.ArrayBuffer(1, 2, 3)).mkString(","))
}
