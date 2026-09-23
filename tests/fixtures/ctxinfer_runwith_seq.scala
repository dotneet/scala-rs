package ctxinfer.runwithseq

object Main {
  def main(args: Array[String]): Unit = {
    val result: scala.concurrent.Future[Seq[Seq[Int]]] =
      new Source[Seq[Int], Unit].runWith(Sink.seq)(null)
    println(result == null)
  }
}
