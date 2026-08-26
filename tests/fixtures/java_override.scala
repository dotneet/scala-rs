class Base {
  def tag: String = "base"
}
class Sub extends Base {
  @Override
  def tag: String = "sub"
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Sub().tag)
    println(new Base().tag)
  }
}
