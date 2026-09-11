// Lambdas are serializable the way nsc makes them (fixture prefix `rtv_`):
// `FunctionN` literals through `altMetafactory` plus `$deserializeLambda$`
// (`run/t10522`, `run/t5018`, `run/t9365`, `run/inlineAddDeserializeLambda`),
// SAM classes pinned at `serialVersionUID = 0` (`run/sammy_seriazable`), and
// `@transient lazy val`s recomputed after a round trip (`run/t10244`,
// `run/t10075`).
import java.io._

trait MkT { def mk(k: Int): Int => Int = (x: Int) => x * k }
object MkO extends MkT
class Adder(val base: Int) extends Serializable {
  def adder: Int => Int = x => x + base
}
trait IntToString extends Serializable { def apply(i: Int): String }
abstract class SerClass extends Serializable { def apply(a: Any): Any }
abstract class PlainClass { def apply(a: Any): Any }
trait LongFun { def f(a: Long, b: Double): Double }

trait HasLazy extends Serializable {
  @transient protected lazy val cache: StringBuilder = new StringBuilder("fresh")
}
class Box extends HasLazy {
  var count = 0
  @transient lazy val t: String = { count += 1; "t" + count }
  lazy val p: String = { count += 10; "p" + count }
  def peek = cache.toString
}

object Main {
  def rt[A](a: A): A = {
    val bos = new ByteArrayOutputStream
    val o = new ObjectOutputStream(bos)
    o.writeObject(a)
    o.close()
    new ObjectInputStream(new ByteArrayInputStream(bos.toByteArray)).readObject().asInstanceOf[A]
  }
  def main(args: Array[String]): Unit = {
    val k = 10
    println(rt((x: Int) => x + k)(5))
    println(rt(MkO.mk(3))(4))
    println(rt(new Adder(10).adder)(5))
    val xs = List(1, 2, 3)
    println(rt((s: String) => s + xs.sum)("n"))
    val its: IntToString = (x: Int) => "yo!" * x
    println(rt(its)(2))
    val sc: SerClass = x => x
    println(rt(sc)("sc"))
    println(ObjectStreamClass.lookup(sc.getClass).getSerialVersionUID)
    val pc: PlainClass = x => x
    println(pc("plain") + " " + pc.getClass.getSuperclass.getSimpleName)
    val lf: LongFun = (a, b) => a * b
    println(lf.f(3L, 1.5))
    val b = new Box
    println(b.t + " " + b.p + " " + b.peek)
    val c = rt(b)
    println(c.t + " " + c.p + " " + c.count + " " + c.peek)
  }
}
