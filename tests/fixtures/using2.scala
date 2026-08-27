class Box(var n: Int) extends AutoCloseable {
  def close(): Unit = { n = n + 1 }
}

object Main {
  def boom(): Box = throw new RuntimeException("acq2")
  def main(args: Array[String]): Unit = {
    import scala.util.Using
    val b = new Box(0)
    val t = Using(b)((x: Box) => 10)
    println(t.getOrElse(0))
    println(b.n)
    val c = new Box(0)
    val t2 = Using(c)((x: Box) => { throw new RuntimeException("boom"); 0 })
    println(t2.getOrElse(-1))
    println(c.n)

    val a1 = new Box(0)
    val a2 = new Box(0)
    val t3 = Using.Manager((mgr) => {
      mgr(a1)
      mgr(a2)
      10
    })
    println(t3.getOrElse(0))
    println(a1.n)
    println(a2.n)

    val c1 = new Box(0)
    val c2 = new Box(0)
    val t4 = Using.Manager((mgr) => {
      mgr(c1)
      mgr(c2)
      throw new RuntimeException("boom")
      0
    })
    println(t4.getOrElse(-1))
    println(c1.n)
    println(c2.n)

    val e = new Box(0)
    val t5 = Using.Manager((mgr) => {
      mgr(e)
      mgr(boom())
      0
    })
    println(t5.getOrElse(-1))
    println(e.n)
  }
}
