class WildBase {
  var inherited: Int = 3
  def plus(x: Int): Int = inherited + x
}
object WildSource extends WildBase { var direct: Int = 1 }
object WildOther extends WildBase
object WildMain {
  def directRead(): Int = { import WildSource._; direct }
  def inheritedRead(): Int = { import WildSource._; inherited }
  def directWrite(): Unit = { import WildSource._; direct = 5 }
  def inheritedWrite(): Unit = { import WildSource._; inherited = 9 }
  def inheritedCall(): Int = { import WildSource._; plus(4) }
  def selectors(): Int = { import WildSource.{direct => renamed, _}; inherited + renamed }
  def nested(): Int = {
    import WildSource._
    val inner = { import WildOther._; plus(1) }
    inner + plus(1)
  }
  def main(args: Array[String]): Unit = {
    println(directRead())
    println(inheritedRead())
    directWrite()
    inheritedWrite()
    println(WildSource.direct)
    println(WildSource.inherited)
    println(inheritedCall())
    println(selectors())
    println(nested())
  }
}
