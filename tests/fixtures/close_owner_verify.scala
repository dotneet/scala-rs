class CloseProbe extends java.io.Closeable {
  var closed: Boolean = false
  override def close(): Unit = { closed = true }
}

object Main {
  def main(args: Array[String]): Unit = {
    val p = new CloseProbe
    try println("body") finally p.close()
    println(p.closed)
  }
}
