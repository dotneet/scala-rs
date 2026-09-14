package vcedge

final class Wrapped(val value: Int) extends AnyVal
final class GenericWrapped[A](val value: A) extends AnyVal

trait Sink[A] {
  def take(x: A): Int
}

trait Mapper[A, B] {
  def apply(x: A): B
}

trait Source {
  def get(): GenericWrapped[String]
}

object Main {
  val arraySink: Sink[Array[Wrapped]] =
    (xs: Array[Wrapped]) => xs.length
  val genericMapper: Mapper[GenericWrapped[String], String] =
    (x: GenericWrapped[String]) => x.value
  val genericSource: Source =
    () => new GenericWrapped[String]("source")

  def main(args: Array[String]): Unit = {
    println(arraySink.take(new Array[Wrapped](3)))
    println(genericMapper(new GenericWrapped[String]("mapped")))
    println(genericSource.get().value)
  }
}
