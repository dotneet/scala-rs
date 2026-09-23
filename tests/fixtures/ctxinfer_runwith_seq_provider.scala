package ctxinfer.runwithseq

import scala.concurrent.Future

trait Materializer
trait SinkShape[A]
trait Graph[S, M]

final class Sink[A, M] extends Graph[SinkShape[A], M]
object Sink {
  def seq[T]: Sink[T, Future[Seq[T]]] = new Sink[T, Future[Seq[T]]]
}

final class Source[A, Mat] {
  def runWith[Mat2](sink: Graph[SinkShape[A], Mat2])(implicit materializer: Materializer): Mat2 =
    null.asInstanceOf[Mat2]
}
