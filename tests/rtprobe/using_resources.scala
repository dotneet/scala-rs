// scala.util.Using: release order of several resources, release on
// exception, and Using.resource returning a primitive; plus a hand-written
// loan pattern with try/finally.
import scala.util.{Using, Try}
object Main {
  val log = scala.collection.mutable.ListBuffer.empty[String]
  class Res(val name: String) extends AutoCloseable { log += s"open $name"; def read: Int = name.length; def close(): Unit = log += s"close $name" }
  def loan[A](name: String)(f: Res => A): A = { val r = new Res(name); try f(r) finally r.close() }
  def main(args: Array[String]): Unit = {
    val t = Using(new Res("one"))(_.read * 10)
    println(t + " " + log.toList); log.clear()
    val t2 = Using.Manager { use => val a = use(new Res("a")); val b = use(new Res("bb")); a.read + b.read }
    println(t2 + " " + log.toList); log.clear()
    val t3 = Using(new Res("boom")) { r => if (r.read > 0) throw new IllegalStateException("inside"); 0 }
    println(t3.isFailure + " " + t3.failed.get.getMessage + " " + log.toList); log.clear()
    val v: Int = Using.resource(new Res("prim"))(_.read)
    println(v + 1 + " " + log.toList); log.clear()
    println(loan("loan")(r => r.name.toUpperCase) + " " + log.toList); log.clear()
    val caught = Try(loan("fail")(_ => throw new RuntimeException("x")))
    println(caught.isFailure + " " + log.toList); log.clear()
    val nested = Using.resources(new Res("r1"), new Res("r2")) { (a, b) => a.name + b.name }
    println(nested + " " + log.toList)
  }
}
