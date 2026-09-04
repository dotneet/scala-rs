// Static forwarders on a *companion class*.
//
// `object Main` next to `class Main` gets no mirror class -- nsc puts the
// object's static forwarders on `Main.class` itself. Without them `java Main`
// cannot start this program at all, since `Main.class` has no `main`.
//
// The `main` below prints every static method `Main.class` really carries, so
// the check covers which members are forwarded as well as that any are: the
// `val`/`var`/`lazy val` accessors, both alternatives of an overload, and the
// `var`'s setter -- but not `private`, `protected` or `private[p]` members,
// and not `kurtz`, whose name the class already uses.
class Main {
  def kurtz: String = "we must incinerate them"
}

object Main {
  val greeting: String = "hello"
  var counter: Int = 1
  lazy val late: Int = 7
  def twice(i: Int): Int = i * 2
  def twice(s: String): String = s + s
  def kurtz: String = "shadowed by the class"
  private def hidden(): Int = -1
  protected def guarded(): Int = -2
  private[Main] def bounded(): Int = -3

  def main(args: Array[String]): Unit = {
    println(new Main().kurtz)
    println(greeting)
    println(counter + late)
    println(twice(21))
    println(twice("ab"))
    val ms = classOf[Main].getDeclaredMethods
    val names = new Array[String](ms.length)
    var n = 0
    var i = 0
    while (i < ms.length) {
      val m = ms(i)
      if (java.lang.reflect.Modifier.isStatic(m.getModifiers())) {
        names(n) = m.getName()
        n += 1
      }
      i += 1
    }
    // Sorted by hand: `getDeclaredMethods` has no defined order, and
    // `java.util.Arrays.sort` on a partly filled `Array[String]` is more
    // library surface than this fixture should depend on.
    i = 1
    while (i < n) {
      val cur = names(i)
      var j = i - 1
      while (j >= 0 && names(j).compareTo(cur) > 0) {
        names(j + 1) = names(j)
        j -= 1
      }
      names(j + 1) = cur
      i += 1
    }
    i = 0
    while (i < n) {
      println(names(i))
      i += 1
    }
  }
}
