import waccsep._
class C extends B { override def g(a: Int = 3): Int = a * 100 }
class K extends T { override def k(s: String = "k"): String = s + "!" }
class M extends T
class N extends T {
  override def k(s: String = "n"): String = s + "N"
  def sup: String = super.k()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new B().g() + " " + (new B: A).g())
    println((new C: A).g() + " " + new C().g())
    println((new K: T).k() + " " + new M().k() + " " + (new M: T).k())
    println(new N().sup + " " + (new N: T).k())
  }
}
