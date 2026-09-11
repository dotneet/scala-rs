// Object initialization is lazy and happens once; nested objects in classes
// are per instance; a cycle between two objects sees default values.
object Main {
  var trace = List.empty[String]
  def t(s: String): Unit = trace = s :: trace

  object O1 { t("O1 init"); val v = { t("O1.v"); 1 } }
  object O2 { t("O2 init"); val w = O1.v + 1 }
  class K(val id: Int) { t(s"K$id ctor"); object Inner { t(s"Inner$id init"); val q = id * 3 } }

  object Cyc1 { val c = 7; val a: Int = Cyc2.b + 1 }
  object Cyc2 { val b: Int = Cyc1.c + 1 }

  object Holder {
    t("Holder init")
    object Deep { t("Deep init"); def hi = "deep" }
    val eager = 5
  }

  class WithCompanion(val x: Int)
  object WithCompanion { t("companion init"); def make = new WithCompanion(9) }

  def main(args: Array[String]): Unit = {
    t("start")
    println(O2.w)
    println(O1.v)
    val k1 = new K(1)
    val k2 = new K(2)
    println(k2.Inner.q)
    println(k1.Inner.q)
    println(k1.Inner.q)
    println(k1.Inner eq k1.Inner)
    try { println(Cyc1.a); println(Cyc2.b) } catch { case e: Throwable => println("cyc " + e.getClass.getName) }
    println(Holder.eager)
    println(Holder.Deep.hi)
    println(WithCompanion.make.x)
    println(trace.reverse.mkString(","))
  }
}
