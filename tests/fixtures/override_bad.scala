class Base {
  def tag: String = "base"
}
class Sub extends Base {
  @Override
  def other: String = "x"
}
object Main {
  def main(args: Array[String]): Unit = ()
}
