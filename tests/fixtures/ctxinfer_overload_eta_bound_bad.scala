trait Base[A] {
  def f[B >: A](x: B): Int
  def f[B >: A](x: B, n: Int): Int
}
object Main {
  def use(base: Base[String]): Int => Int = base.f
}
