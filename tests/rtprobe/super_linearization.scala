// Stackable traits: abstract override, super chains through the
// linearization, and the order in which mixins are composed.
object Main {
  abstract class Queue { def put(x: Int): Unit; def get(): Int; def dump: String }
  class BasicQueue extends Queue {
    private val buf = scala.collection.mutable.ArrayBuffer.empty[Int]
    def put(x: Int): Unit = buf += x
    def get(): Int = buf.remove(0)
    def dump = buf.mkString("[", ",", "]")
  }
  trait Doubling extends Queue { abstract override def put(x: Int): Unit = super.put(2 * x) }
  trait Incrementing extends Queue { abstract override def put(x: Int): Unit = super.put(x + 1) }
  trait Filtering extends Queue { abstract override def put(x: Int): Unit = if (x >= 0) super.put(x) }

  trait Base { def name: String = "Base" }
  trait L1 extends Base { override def name: String = "L1(" + super.name + ")" }
  trait L2 extends Base { override def name: String = "L2(" + super.name + ")" }
  trait L3 extends L1 { override def name: String = "L3(" + super.name + ")" }
  class Mixed1 extends L1 with L2
  class Mixed2 extends L2 with L1
  class Mixed3 extends L2 with L3
  class Mixed4 extends L3 with L2 { override def name: String = "M4(" + super.name + ")" }

  trait Logger { def log(s: String): String = s }
  trait Stamp extends Logger { override def log(s: String): String = super.log("[stamp] " + s) }
  trait Upper extends Logger { override def log(s: String): String = super.log(s.toUpperCase) }

  def main(args: Array[String]): Unit = {
    val q1 = new BasicQueue with Doubling with Incrementing
    q1.put(1); q1.put(-3); q1.put(10)
    println(q1.dump)
    val q2 = new BasicQueue with Incrementing with Doubling
    q2.put(1); q2.put(-3); q2.put(10)
    println(q2.dump)
    val q3 = new BasicQueue with Incrementing with Filtering
    q3.put(-1); q3.put(-2); q3.put(4)
    println(q3.dump + " get=" + q3.get() + " after=" + q3.dump)
    println(new Mixed1().name)
    println(new Mixed2().name)
    println(new Mixed3().name)
    println(new Mixed4().name)
    println((new Logger with Stamp with Upper).log("msg"))
    println((new Logger with Upper with Stamp).log("msg"))
  }
}
