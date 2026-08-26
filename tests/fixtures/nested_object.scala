object Outer {
  object Inner {
    def hi(): String = "nested"
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Outer.Inner.hi())
  }
}
