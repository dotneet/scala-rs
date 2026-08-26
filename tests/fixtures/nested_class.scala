class Outer {
  class Inner {
    def hi(): String = "inner"
  }
  def make(): String = new Inner().hi()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer().make())
  }
}
