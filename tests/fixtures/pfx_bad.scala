// Invalid neighbours of tests/fixtures/pfx_inner.scala: every line marked
// `// error` is rejected by scalac 2.13.16, and the test requires scala-rs to
// reject exactly those lines.
class P {
  trait S1
  val p: P = null
  trait S2 { def f(x: p.S1): Int }
  val s: p.S1 = new S1 {}                                    // error: P.this.S1 is not a p.S1
  class C extends S1
  val c: p.S1 = new C                                        // error: C extends P.this.S1
}
class MailBox { class Message }
abstract class Actor {
  private val in = new MailBox
  def send(msg: in.Message) = sys.error("foo")
  def unstable: Actor = sys.error("foo")
  def dubiousSend(msg: MailBox#Message): Nothing = unstable.send(msg) // error: MailBox#Message is not an in.Message
}
class Outer[T](val x: T) { class In { def get: T = x }; def mk = new In }
trait MySet[K]
trait MapOps[K] { class KeySet extends MySet[K] }
class HM[K] extends MapOps[K]
trait Fixed { class Inner }
object Main {
  def g(i: Outer[Int]#In): Int = i.get
  def dep(o: Outer[String])(i: o.In): String = i.get
  val a = new Outer("a")
  val b = new Outer("b")
  val i: a.In = new a.In
  val j: b.In = i                                            // error: a.In is not a b.In
  val k: Outer[String]#In = new a.In
  val l: a.In = k                                            // error: Outer[String]#In is not an a.In
  val m: a.In = b.mk                                         // error: b.mk is a b.In
  val n = g(new a.In)                                        // error: a.In is not an Outer[Int]#In
  val d = dep(a)(new b.In)                                   // error: b.In is not an a.In
  val hm = new HM[Int]
  val ks: MySet[String] = new hm.KeySet                      // error: hm.KeySet is a MySet[Int]
  val f = new Fixed {}.Inner                                 // error: value Inner is not a member of Fixed
}
