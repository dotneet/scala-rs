object Main {
  def go(): Unit = println(1)
  def cmp(a: Int, b: Int): Int = a - b
  def main(args: Array[String]): Unit = {
    val r: Runnable = () => println(2)
    r.run()
    val r2: Runnable = go _
    r2.run()
    val c: java.util.Comparator[Int] = (a, b) => a - b
    println(c.compare(3, 1))
    val c2: java.util.Comparator[Int] = cmp
    println(c2.compare(1, 3))
    val f: java.util.function.Function[Int, Int] = (x) => x + 1
    println(f.apply(40))
  }
}
