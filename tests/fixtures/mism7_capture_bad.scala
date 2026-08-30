// The enclosing method's parameter keeps *its* type inside the anonymous
// class, so passing the anonymous class's own element where the outer `T` is
// wanted is still a mismatch.
trait It[T] { self =>
  def next(): T
  def map[B](f: T => B): It[B] = new It[B] {
    def next(): B = f(this.next())
  }
}
object Main {
  def main(args: Array[String]): Unit = ()
}
