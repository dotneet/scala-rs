trait Bound { def n: Int }
class BI extends Bound { def n: Int = 41 }
trait T { type A <: Bound; def x: A }
class C extends T { type A = BI; def x: A = new BI }
abstract class D { type A <: Int; def x: A }
class E extends D { type A = Int; def x: A = 41 }
abstract class Lo { type A >: Null; def y: A }
class LoOk extends Lo { type A = String; def y: A = "ok" }
object Main {
  def fromC(c: C): Int = c.x.n
  def fromE(e: E): Int = e.x
  def asBound(t: T): Bound = t.x
  def asInt(d: D): Int = d.x
  def main(args: Array[String]): Unit = {
    println(fromC(new C))
    println(fromE(new E))
    println(asBound(new C).n)
    println(asInt(new E))
    println(new LoOk().y)
  }
}
