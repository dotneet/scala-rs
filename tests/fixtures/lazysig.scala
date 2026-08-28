// A member without a type annotation only gets its signature once its
// right-hand side is typed. Every reference below is reached before the
// definition it names, so each one has to complete that definition on demand
// (nsc's lazy completers).
class Client extends Log {
  def total: Int = Store.base + Store.doubled
  def tag: String = prefix + Store.name
  def shout: String = stamp + tag
  private[this] lazy val stamp = "[" + Store.name + "]"
}

trait Log {
  def prefix = "log:"
}

class Counter {
  def report: String = label + ":" + step
  private val step = 7
  lazy val label = "c" + step
}

// Signature side effects (default getters, type parameters) must not be
// synthesized twice when the signature is completed early.
object Uses {
  def viaDefault: Int = Store.scaled() + Store.scaled(2)
  def viaGeneric: Int = Store.pick(4, 5)
}

object Store {
  val base = 20
  def doubled = base * 2
  lazy val name = "store"
  def scaled(by: Int = 3) = base * by
  def pick[A](a: A, b: A) = b
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Client
    println(c.total)
    println(c.tag)
    println(c.shout)
    println(Store.doubled)
    println(Store.name.length)
    println(new Counter().report)
    println(Uses.viaDefault)
    println(Uses.viaGeneric)
  }
}
