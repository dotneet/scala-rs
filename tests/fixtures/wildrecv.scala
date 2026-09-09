class WildBase {
  var inherited: Int = 3
  def plus(x: Int): Int = inherited + x
}
object WildSource extends WildBase { var direct: Int = 1 }
object WildFactory {
  case class Payload(value: Int)
  object Payload {
    def apply(text: String, count: Int): Payload = new Payload(text.length + count)
  }
}
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
  def companion(): Int = {
    import WildFactory._
    Payload(text = "abc", count = 4).value
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
    println(companion())
  }
}
