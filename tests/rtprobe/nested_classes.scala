// Inner classes keep their outer instance; local classes capture locals;
// anonymous classes capture both; objects nested in classes per instance.
object Main {
  class Outer(val tag: String) {
    private var hits = 0
    class Inner(val n: Int) {
      def describe = { hits += 1; s"$tag/$n/hits=$hits" }
      class Deeper { def path = tag + "." + n + ".deep" }
    }
    def make(n: Int) = new Inner(n)
    object Registry { var names = List.empty[String]; def add(s: String) = { names ::= tag + s; this } }
    def hitsNow = hits
  }

  trait Shape { def area: Int }

  def local(k: Int): List[String] = {
    val base = k * 2
    class Loc(val m: Int) { def calc = base + m + k }
    case class LC(a: Int)
    val anon = new Shape { def area = base * base }
    List(new Loc(1).calc.toString, LC(base).toString, anon.area.toString)
  }

  def main(args: Array[String]): Unit = {
    val o1 = new Outer("o1"); val o2 = new Outer("o2")
    val i1 = o1.make(1); val i2 = new o2.Inner(2)
    println(i1.describe); println(i1.describe); println(i2.describe)
    println(o1.hitsNow + " " + o2.hitsNow)
    val d = new i1.Deeper
    println(d.path)
    o1.Registry.add("a").add("b"); o2.Registry.add("c")
    println(o1.Registry.names + " " + o2.Registry.names)
    println(local(3))
    println(local(5))
    var counter = 0
    val anons = (1 to 3).map { i => new Shape { def area = { counter += i; i * i } } }
    println(anons.map(_.area).sum + " counter=" + counter)
    // an inner class instance from a different outer is still tied to its own
    val xs = List(o1.make(7), o2.make(8))
    println(xs.map(_.describe).mkString(" "))
  }
}
