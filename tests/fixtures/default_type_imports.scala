trait Alternative[F[_]] {
  def pure[A](a: A): F[A]
}
object Main {
  val warm = scala.collection.immutable.Stream.apply[Int](0)
  val instance: Alternative[Stream] = new Alternative[Stream] {
    def pure[A](a: A): Stream[A] = Stream(a)
  }
  def main(args: Array[String]): Unit = {
    println(instance.pure(1).toList)
    println(instance.pure("a").toList)
  }
}
