final class SeqBox[A](val toSeq: scala.collection.immutable.Seq[A]) extends AnyVal

object Main {
  def tail(v: SeqBox[Either[Int, Int]]): Option[SeqBox[Either[Int, Int]]] = {
    val rest = v.toSeq.tail
    if (rest.nonEmpty) Some(new SeqBox(rest)) else None
  }

  def main(args: Array[String]): Unit = {
    val v = new SeqBox[Either[Int, Int]](List(Left(1), Right(2)))
    tail(v) match {
      case Some(next) => println(next.toSeq.head)
      case None => println("none")
    }
  }
}
