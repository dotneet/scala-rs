object Main {
  def use(f: Int => String): String = {
    val any = f.asInstanceOf[Any => Any]
    val back = any.asInstanceOf[Int => String]
    back(7) + " " + f.isInstanceOf[Int => String]
  }
  def main(args: Array[String]): Unit = {
    println(use(n => "n" + n))
    val g: (Int, Int) => Int = _ + _
    println(g.asInstanceOf[Any].toString.nonEmpty)
  }
}
