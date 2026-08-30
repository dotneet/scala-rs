class Base {
  def tag: String = "base"
}
class Sub extends Base {
  // SLS 5.1.4: redefining a concrete member needs the `override` *modifier*.
  // Java's `@Override` is an annotation and does not stand in for it -- real
  // scalac 2.13.16 rejects this file without the keyword
  // (``override` modifier required to override concrete member`). The file
  // only compiled before because scala-rs had no override check at all.
  @Override
  override def tag: String = "sub"
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Sub().tag)
    println(new Base().tag)
  }
}
