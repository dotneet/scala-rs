// A pattern reads the enclosing instance through its extractor.
//
// `for (t @ c.TT() <- ts) if (…) …` desugars to
// `ts.withFilter{ case t @ c.TT() => true; case _ => false }.foreach(…)`, and the
// predicate's only free reference to `this` is *inside* the pattern -- under a
// `Bind`, which the free-variable walk did not descend into. The body method was
// then emitted with no outer parameter and `aload_0` read the list element as
// the enclosing class:
//
//   java.lang.ClassCastException: class java.lang.String cannot be cast to
//   class Lifter
//
// cats hit this inside its own macro, so real scalac threw while expanding
// `FunctionK.lift` (`cats/arrow/FunctionKMacros.scala`'s
// `for (typeArg @ TypeTree() <- typeArgs) if (typeArg.original != null)`).
trait Ctx {
  type T
  val TT: TTExtractor
  trait TTExtractor { def unapply(t: T): Boolean }
  def mk(n: Int): T
  def show(t: T): String
}

class C1 extends Ctx {
  type T = String
  val TT: TTExtractor = new TTExtractor {
    def unapply(t: String): Boolean = t.startsWith("a")
  }
  def mk(n: Int): String = if (n % 2 == 0) "a" + n else "b" + n
  def show(t: String): String = t
}

class Lifter[C <: Ctx](val c: C) {
  val tag = "tag:"

  /// A `Bind` over an extractor that is a member of `this`.
  def filtered(ts: List[c.T]): List[String] = {
    val out = scala.collection.mutable.ListBuffer[String]()
    for (t @ c.TT() <- ts) if (c.show(t) != "") out += c.show(t)
    out.toList
  }

  /// `Alternative` in a pattern, whose second branch reads `this`.
  def alts(xs: List[Any]): List[String] =
    (for (y @ (`tag` | "keep") <- xs) yield y.toString).toList

  /// `Star`: a varargs pattern whose element reads `this`.
  def stars(xs: List[Seq[String]]): List[String] =
    (for (Seq(`tag`, rest @ _*) <- xs) yield rest.mkString("|")).toList
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new C1
    val l = new Lifter[C1](c)
    println(l.filtered(List(c.mk(0), c.mk(1), c.mk(2), c.mk(3))))
    println(l.alts(List("tag:", "drop", "keep", 5)))
    println(l.stars(List(Seq("tag:", "a", "b"), Seq("no", "c"), Seq("tag:"))))
  }
}
