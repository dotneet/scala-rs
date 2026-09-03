// By-name parameters, lazy vals in a class and in a method body, and Option
// chaining -- the shape slick uses for lazily-built session state.
object Main {
  var log = List.empty[String]
  def note(s: String): Unit = { log = s :: log }

  def once[A](body: => A): () => A = {
    lazy val v = { note("forced"); body }
    () => v
  }

  class Config(raw: Map[String, String]) {
    lazy val url: Option[String] = { note("url"); raw.get("url") }
    lazy val port: Int = { note("port"); raw.get("port").map(_.toInt).getOrElse(5432) }
    lazy val label: String = url.map(_ + ":" + port).getOrElse("none")
  }

  def orElseChain(m: Map[String, String]): String =
    m.get("a").orElse(m.get("b")).orElse(Some("z")).map(_.toUpperCase).getOrElse("?")

  def timed[A](name: String)(body: => A): A = {
    note("enter " + name)
    val r = body
    note("exit " + name)
    r
  }

  def cond(p: Boolean, t: => Int, f: => Int): Int = if (p) t else f

  def main(args: Array[String]): Unit = {
    val f = once { note("body"); 41 + 1 }
    println(log.reverse)
    println(f())
    println(f())
    println(log.reverse)

    log = Nil
    val c = new Config(Map("url" -> "jdbc:x", "port" -> "999"))
    println(log.reverse)
    println(c.label)
    println(c.label)
    println(log.reverse)

    println(orElseChain(Map("b" -> "hey")))
    println(orElseChain(Map()))
    println(timed("t")(1 + 2))
    println(log.reverse.filter(_.startsWith("e")))
    println(cond(true, 1, sys.error("boom")))
    println(cond(false, sys.error("boom"), 2))

    lazy val a: Int = { note("a"); b + 1 }
    lazy val b: Int = { note("b"); 10 }
    log = Nil
    println(a + a)
    println(log.reverse)

    val opts = List(Some(1), None, Some(3))
    // `opts.flatten` is a known gap: `IterableOps.flatten[B](implicit
    // toIterableOnce: A => IterableOnce[B])` is not applied. See README.
    println(opts.flatMap(x => x).sum)
    println(opts.collect { case Some(x) if x > 1 => x })
    println(for { x <- Some(2); y <- Some(3) if y > 1 } yield x * y)
    println(Option("s").fold(0)(_.length))
    println((None: Option[String]).fold(0)(_.length))
  }
}
