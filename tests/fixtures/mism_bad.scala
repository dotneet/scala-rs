// The widening that used to hide behind `relax_open_tparams` must not come
// back as permissiveness: an invariant class really does reject a different
// type argument, and `Inv[Int]` is not an `Inv[Any]`.
class Inv[T](val value: T)

object Main {
  def bad(a: Inv[Int]): Inv[Any] = a

  def main(args: Array[String]): Unit = println(bad(new Inv(1)).value)
}
