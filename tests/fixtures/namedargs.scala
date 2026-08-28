// Named arguments and default arguments: reordering at method, `apply`,
// `copy`, constructor and overloaded call sites, plus the default-argument
// combinations that go with them.

case class Conf(host: String, port: Int = 80, secure: Boolean = false)

class Server(val name: String = "srv", val threads: Int = 4, val debug: Boolean = false) {
  def show: String = name + "/" + threads + "/" + debug
}

class Pick {
  def p(a: Int, b: Int): String = "ii:" + a + "," + b
  def p(x: String, y: String): String = "ss:" + x + "," + y
}

class Base {
  def info: Conf = Conf("base", 1, secure = true)
}

class Sub extends Base {
  override def info: Conf = super.info.copy(port = 2)
}

object Api {
  def apply(x: Int, y: Int = 3): Int = x * 10 + y
  def area(width: Int, height: Int): Int = width * height
  def opts(a: Int, b: Int = 2, c: Int = 3): Int = a * 100 + b * 10 + c
  def curried(a: Int, b: Int = 1)(c: Int, d: Int = 2): Int = a * 1000 + b * 100 + c * 10 + d
  def dep(a: Int)(b: Int = a + 1): Int = a * 10 + b
  def tagged(first: Int, rest: Int*): Int = first * 100 + rest.sum
}

object Main {
  def main(args: Array[String]): Unit = {
    // Plain reordering.
    println(Api.area(height = 3, width = 4))
    // A named argument already at its own position leaves the rest positional.
    println(Api.area(width = 4, 3))

    // Defaults combined with names.
    println(Api.opts(1))
    println(Api.opts(1, c = 9))
    println(Api.opts(c = 9, a = 1))
    println(Api.opts(b = 5, a = 1))

    // Companion `apply`.
    println(Api(y = 4, x = 1))

    // Defaults in a later parameter clause.
    println(Api.curried(b = 9, a = 1)(d = 7, c = 2))
    println(Api.curried(1)(2))
    println(Api.dep(4)())

    // A repeated parameter takes no argument at all.
    println(Api.tagged(first = 1))
    println(Api.tagged(first = 1, 2, 3))

    // Case class `apply` and `copy`.
    val c = Conf(host = "h")
    println(c)
    println(c.copy(port = 8080))
    println(Conf(secure = true, host = "x", port = 1))
    println(new Sub().info)

    // Constructor named arguments and constructor defaults.
    val s = new Server(threads = 8)
    println(s.show)
    println(new Server(debug = true, name = "n").show)
    println(new Server().show)

    // Overloads are narrowed by parameter name before argument type.
    val pk = new Pick
    println(pk.p(b = 1, a = 2))
    println(pk.p(y = "1", x = "2"))
  }
}
