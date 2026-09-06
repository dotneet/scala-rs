object Main {
  def pure[A](a: A): scala.collection.immutable.Stream[A] = scala.collection.immutable.Stream.apply(a)
  def main(args: Array[String]): Unit = {
    println(pure(1).toList)
    println(pure("a").toList)
  }
}
