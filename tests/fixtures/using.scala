class Box(var n: Int) extends AutoCloseable {
  def close(): Unit = { n = n + 1 }
}

object Main {
  def main(args: Array[String]): Unit = {
    import scala.util.Using
    val b = new Box(0)
    val r = Using.resource(b)((x: Box) => 10)
    println(r)
    println(b.n)
    val c = new Box(0)
    try {
      Using.resource(c)((x: Box) => {
        throw new RuntimeException("boom")
        ()
      })
    } catch {
      case _: RuntimeException => println("caught")
    }
    println(c.n)
  }
}
