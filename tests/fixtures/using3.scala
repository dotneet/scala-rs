class Box(var n: Int) extends AutoCloseable {
  def close(): Unit = { n = n + 1 }
}

object Main {
  def main(args: Array[String]): Unit = {
    import scala.util.Using
    val a = new Box(0)
    val b = new Box(0)
    val r = Using.resources(a, b)((x: Box, y: Box) => 10)
    println(r)
    println(a.n)
    println(b.n)
    val c = new Box(0)
    val d = new Box(0)
    try {
      Using.resources(c, d)((x: Box, y: Box) => {
        throw new RuntimeException("boom")
        0
      })
    } catch {
      case _: RuntimeException => println("caught")
    }
    println(c.n)
    println(d.n)
  }
}
