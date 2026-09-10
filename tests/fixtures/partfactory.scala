object Main {
  implicit val app: PureTC[Option] = new PureTC[Option] {
    def pure[A](a: A): Option[A] = Some(a)
  }
  def lifted[F[_], A](a: A)(implicit F: PureTC[F]): Wrap[F, A] = Wrap.pure(a)
  def mono[F[_]](a: Int)(implicit F: PureTC[F]): Wrap[F, Int] = Wrap.mono(a)
  def value[F[_], A](a: A)(implicit F: PureTC[F]): Wrap[F, A] = Wrap.value(a)
  def main(args: Array[String]): Unit = {
    val first: Cell[String] = Factory.make("one")
    val second: Cell[Int] = Factory.make(2)
    val third = Factory.make("three")
    println(Factory.lower(3).value)
    println(first.value)
    println(second.value)
    println(third.value.length)
    val bounded: Cell[java.lang.Integer] = Factory.bounded(java.lang.Integer.valueOf(7))
    println(bounded.value.intValue())
    println(lifted[Option, Int](3).value)
    println(mono[Option](4).value)
    println(value[Option, String]("five").value)
  }
}
