// A repeated parameter used as a value: `xs: T*` is a
// `scala.collection.immutable.Seq[T]` inside the body, whatever the name `Seq`
// means where the method is written.
object Main {
  // The two ways a program can bind the name `Seq` to something of its own.
  class Seq[A](val tag: String)
  type Alias[+A] = scala.collection.immutable.Seq[A]

  def ints(xs: Int*): String = xs.mkString(",") + "/" + xs.length
  def shorts(xs: Short*): Int = { var t = 0; xs.foreach(s => t += s.toInt); t }
  def units(xs: Unit*): String = xs.length.toString + ":" + xs.mkString("|")
  def gen[T](xs: T*): String = xs.mkString("[", " ", "]")
  def arrs(xs: Array[Int]*): String = {
    var t = 0
    xs.foreach(a => t += a.length)
    t.toString + "/" + xs.map(a => a.length).mkString(".")
  }
  def forward(xs: Int*): String = ints(xs: _*)
  def alias(xs: Alias[Int]): String = ints(xs: _*)

  def main(args: Array[String]): Unit = {
    println(ints(1, 2, 3))
    println(ints())
    println(shorts(1.toShort, 2.toShort))
    println(units((), ()))
    println(gen("a", "b"))
    println(arrs(Array(1, 2), Array(3)))
    println(forward(4, 5))
    println(alias(List(6, 7)))
    println(new Seq[Int]("mine").tag)
  }
}
