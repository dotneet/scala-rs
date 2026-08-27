object Main {
  def once(thunk: => Int): Int = thunk
  def main(args: Array[String]): Unit = {
    var n = 0
    val inc = () => { n += 1; n }
    println(inc())
    println(n)
    var s = "a"
    val app = () => { s = s + "b"; s }
    println(app())
    println(s)
    var m = 0
    def bump(): Int = { m += 1; m }
    println(bump())
    println(m)
    var k = 0
    println(once { k += 1; k })
    println(k)
  }
}
