trait SelfValue[T] extends Any { def self: Wrapped[T] }
class Wrapped[T](val value: T) extends AnyVal with SelfValue[T] {
  def self: Wrapped[T] = this
  def applied(): Wrapped[T] = this
  def blocked: Wrapped[T] = { val ignored = 0; this }
  def asAny: Any = this
}
class TextValue(val value: String) extends AnyVal {
  def self: TextValue = this
  def asAny: Any = this
}
class IntValue(val value: Int) extends AnyVal {
  def self: IntValue = this
  def asAny: Any = this
}
object Main {
  def main(args: Array[String]): Unit = {
    val i = new Wrapped(2)
    val parent: SelfValue[Int] = i
    println(parent.self.value)
    println(i.applied().value)
    println(i.blocked.value)
    println(i.asAny.asInstanceOf[Wrapped[Int]].value)
    val s = new Wrapped("generic")
    println(s.self.value)
    println(s.applied().value)
    println(s.asAny.asInstanceOf[Wrapped[String]].value)
    val text = new TextValue("text")
    println(text.self.value)
    println(text.asAny.asInstanceOf[TextValue].value)
    val n = new IntValue(7)
    println(n.self.value)
    println(n.asAny.asInstanceOf[IntValue].value)
  }
}
