trait SP[T] { def name: String }
class Tagged[T](val name: String) extends SP[T]
class Bag(val n: Int, val children: SP[_]*) {
  def describe: String = n + ":" + children.map(_.name).mkString(",")
}
class Nums(val xs: Int*) { def total: Int = xs.sum }
object Main {
  def main(args: Array[String]): Unit = {
    println(new Bag(1, new Tagged[Int]("a"), new Tagged[String]("b")).describe)
    println(new Bag(2).describe)
    println(new Nums(1, 2, 3).total)
    println(new Nums().total)
  }
}
