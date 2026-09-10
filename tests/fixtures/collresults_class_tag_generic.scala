import scala.collection.immutable.ArraySeq
import scala.reflect.ClassTag
object Main {
  def convert[A: ClassTag](xs: List[A]): ArraySeq[A] = xs.to(ArraySeq)
  def main(args: Array[String]): Unit = {
    val ints: ArraySeq[Int] = convert(List(1, 2, 3))
    val words: ArraySeq[String] = convert(List("a", "b"))
    val arrays: ArraySeq[Array[Int]] = convert(List(Array(1, 2), Array(3)))
    println(ints.mkString(","))
    println(words.mkString(","))
    println(arrays.iterator.map(_.sum).mkString(","))
  }
}
