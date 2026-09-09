trait Base[A] { def ordering: Ordering[A] }
class IntBase extends Base[Int] {
  def ordering = (x: Int, y: Int) => java.lang.Integer.compare(x, y)
}
class OptionBase[A](base: Base[A]) extends Base[Option[A]] {
  def ordering = (x: Option[A], y: Option[A]) =>
    if (x.isEmpty && y.isEmpty) 0
    else if (x.isEmpty) -1
    else if (y.isEmpty) 1
    else base.ordering.compare(x.get, y.get)
}
object Main {
  def main(args: Array[String]): Unit = {
    val base: Base[Int] = new IntBase
    val opt: Base[Option[Int]] = new OptionBase(base)
    println(base.ordering.compare(1, 2))
    println(base.ordering.compare(2, 1))
    println(opt.ordering.compare(None, Some(1)))
    println(opt.ordering.compare(Some(1), Some(2)))
  }
}
