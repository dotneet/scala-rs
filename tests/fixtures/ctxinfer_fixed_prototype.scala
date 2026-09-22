trait Input[-A]
trait Graph[+S, +M] {
  def materialize(value: Any): M
}
final class Result[+A](val value: A) {
  def map[B](f: A => B): Result[B] = new Result(f(value))
}
final class Sink[-A, +M](consume: A => M) extends Graph[Input[A], M] {
  def materialize(value: Any): M = consume(value.asInstanceOf[A])
}
object Sink {
  def fold[S, A](zero: S)(f: (S, A) => S): Sink[A, Result[S]] =
    new Sink(a => new Result(f(zero, a)))
}
final class Source[+A](value: A) {
  def runWith[M](sink: Graph[Input[A], M]): M = sink.materialize(value)
}
final case class Text(value: String) {
  def append(other: Text): Text = Text(value + other.value)
}
object Main {
  def main(args: Array[String]): Unit = {
    val source = new Source(Text("value"))
    val result = source.runWith(Sink.fold(Text("prefix:"))(_.append(_))).map(_.value)
    println(result.value)
  }
}
