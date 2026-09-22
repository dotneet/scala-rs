trait Input[-A]
trait Graph[+S, +M]
final class Sink[-A, +M] extends Graph[Input[A], M]
object Sink {
  def fold[S, A](zero: S)(f: (S, A) => S): Sink[A, S] = new Sink[A, S]
}
final class Source[+A] {
  def runWith[M](sink: Graph[Input[A], M]): M = ???
}
final class Text { def append(other: Text): Text = this }
object Main {
  val result = new Source[Int].runWith(Sink.fold(new Text)(_.append(_))).toString
}
