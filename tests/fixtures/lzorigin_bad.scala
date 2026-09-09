import scala.collection.immutable.ArraySeq
object Bad {
 def zip(a: Seq[Int], b: Seq[Int]) = a.lazyZip(b)
 def wrong(a:ArraySeq[Int], b:ArraySeq[Int]):ArraySeq[String] = a.lazyZip(b).map(_ + _)
}
