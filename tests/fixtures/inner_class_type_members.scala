// An inner class of a trait uses the trait's abstract type member; the
// class that fixes the member with an alias instantiates the inner class
// and reads its members, unqualified and through a path.
trait CompA { type KA; def mk: KA; final class InnerA(val k: KA) { def get: KA = k }; def mkI(i: KA): InnerA = new InnerA(i) }
object AppA extends CompA {
  type KA = Int; def mk: KA = 5
  def x: Int = new InnerA(mk).get
  def y: Int = { val q: InnerA = new InnerA(4); q.k + q.get }
}
class AppS extends CompA { type KA = String; def mk = "s"; def v: String = new InnerA(mk).get + mkI("t").k }

// The cake shape: two self-typed components whose inner class pairs their
// members.
trait CompB { self: CompC => type KB; def mkB: KB; final class InnerB(val k: KB) { def pair = (k, mkC) } }
trait CompC { self: CompB => type KC; def mkC: KC }
object Cake extends CompB with CompC {
  type KB = Int; def mkB: KB = 1
  type KC = String; def mkC: KC = "c"
  def u: (Int, String) = new InnerB(mkB).pair
}

object Main {
  def main(args: Array[String]): Unit = {
    val a: Int = AppA.mk
    val b: Int = AppA.mkI(3).get
    val c: Int = AppA.mkI(4).k
    println((a, b, c, AppA.x, AppA.y, new AppS().v, Cake.u))
  }
}
