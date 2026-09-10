trait PureTC[F[_]] { def pure[A](a: A): F[A] }
case class Wrap[F[_], A](value: F[A])
object Wrap {
  class Part[F[_]] {
    def apply[A](a: A)(implicit F: PureTC[F]): Wrap[F, A] = Wrap(F.pure(a))
  }
  class Mono[F[_]] {
    def apply(a: Int)(implicit F: PureTC[F]): Wrap[F, Int] = Wrap(F.pure(a))
  }
  class ValuePart[F[_]](val dummy: Boolean) extends AnyVal {
    def apply[A](a: A)(implicit F: PureTC[F]): Wrap[F, A] = Wrap(F.pure(a))
  }
  def pure[F[_]]: Part[F] = new Part[F]
  def mono[F[_]]: Mono[F] = new Mono[F]
  def value[F[_]]: ValuePart[F] = new ValuePart[F](true)
}

case class Cell[A](value: A)
object Factory {
  class Part[A] { def apply(a: A): Cell[A] = Cell(a) }
  def make[A]: Part[A] = new Part[A]
  def lower[A >: String]: Part[A] = new Part[A]
  def bounded[A <: java.lang.Number]: Part[A] = new Part[A]
}
