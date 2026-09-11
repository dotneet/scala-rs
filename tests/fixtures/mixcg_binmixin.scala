// Mixin forwarders and super accessors for traits read from the
// scala-library *jar*, and the cast a trait-typed value needs where the
// class it extends is expected.
import scala.collection.mutable.{ArrayBuffer, Buffer}
import scala.util.control.NoStackTrace

// `NoStackTrace.fillInStackTrace` overrides `Throwable`'s: without the
// forwarder the class method wins and the stack trace is filled in.
class Quiet extends RuntimeException("quiet") with NoStackTrace
class QuietNse extends NoSuchElementException("nse") with NoStackTrace

// `SeqOps.sizeCompare` calls `super.sizeCompare`: the object owes the
// `SeqOps$$super$sizeCompare` accessor (`AbstractMethodError` without it).
object Five extends Seq[Int] {
  def apply(i: Int) = i
  def length = 5
  def iterator = Iterator(0, 1, 2, 3, 4)
}

// A stackable trait over a library class: `super.insertAll` resolves to
// `ArrayBuffer`'s own, read from its class file.
trait Logged[A] extends Buffer[A] {
  var log = List.empty[String]
  abstract override def insertAll(n: Int, xs: IterableOnce[A]): Unit = {
    log = s"insertAll@$n" :: log
    super.insertAll(n, xs)
  }
}

// Library traits the symbol table's linearization leaves out or misorders:
// `SeqOps$$super$concat` / `$$super$sizeCompare` are still owed, and resolve
// to `IterableOps` (pos/t3568's shape, with and without an explicit
// `StrictOptimizedSeqOps`).
abstract class PlainSeq[E] extends collection.IndexedSeq[E] { def length = 1 }
class OnePlain extends PlainSeq[String] { def apply(i: Int) = "p" }
abstract class StrictSeq[E] extends collection.IndexedSeq[E]
  with collection.StrictOptimizedSeqOps[E, collection.IndexedSeq, collection.IndexedSeq[E]] {
  def length = 1
}
class OneStrict extends StrictSeq[String] { def apply(i: Int) = "s" }

// A trait extending a class is an interface to the JVM: every place a
// value of the trait's type meets the class needs a cast.
trait Mute extends Throwable { override def fillInStackTrace(): Throwable = this }
class MuteEx extends RuntimeException("mute") with Mute

class C0 { def name: String = "c0" }
trait T0 extends C0 {
  override def name: String = "t0"
  def asC: C0 = this
  val fieldC: C0 = this
  def boom(): Nothing = throw new RuntimeException("x") with Mute0
}
trait Mute0 extends RuntimeException
class D0 extends C0 with T0
class Holder(val c: C0) { var v: C0 = null }
class Sub(t: T0) extends Holder(t)

object Main {
  def take(c: C0): String = "take:" + c.name
  def pick(b: Boolean, t: T0, c: C0): C0 = if (b) t else c
  def mt(x: Any, t: T0): C0 = x match { case 1 => t; case _ => new C0 }

  def main(args: Array[String]): Unit = {
    println(new Quiet().getStackTrace.length)
    println(new QuietNse().getStackTrace.length)
    println(new MuteEx().getStackTrace.length)
    println(Five.lengthCompare(7) < 0)
    println(Five.lengthCompare(5) == 0)

    val b = new ArrayBuffer[Int] with Logged[Int]
    b ++= Seq(1, 2)
    b.insertAll(1, Seq(9, 8))
    println(b.toList + " " + b.log)

    val t: T0 = new D0
    println(t.asC.name + " " + t.fieldC.name + " " + take(t))
    val c: C0 = t
    val h = new Holder(t)
    h.v = t
    println(c.name + " " + h.c.name + " " + h.v.name + " " + new Sub(t).c.name)
    println(pick(true, t, null).name + " " + mt(1, t).name + " " + mt(2, t).name)
    var loc: C0 = new C0
    loc = t
    println(loc.name + " " + (try t catch { case _: Throwable => new C0 }).name)
    try t.boom() catch { case e: Mute0 => println("caught " + e.getMessage) }
    val op = new OnePlain
    val os = new OneStrict
    println(op.sizeCompare(3) + " " + op.concat(List("x")).toList)
    println(os.sizeCompare(1) + " " + os.concat(List("y")).toList)
  }
}
