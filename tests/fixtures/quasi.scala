// Groundwork for quasiquotes (`q"..."`), all of it reached on the way to
// `scala.reflect`'s universe API. See `docs/macros.md` §6.2 and
// `crates/cli/tests/quasi.rs`.
//
//  * a member of a *package object* read from a jar -- `scala.math.Pi` is a
//    `val` on `scala/math/package$`, and the typer folds it into the package,
//    which has no runtime value of its own;
//  * `import <a value>._`, which is how `import c.universe._` and
//    `import scala.reflect.runtime.universe._` bring `Tree` / `TermName` /
//    `Literal` into scope;
//  * applying a *parameterless* `def` whose result has an `apply`, which is
//    the shape of every extractor in the reflection API
//    (`def Literal: LiteralExtractor`, then `Literal(...)`).

import scala.language.implicitConversions

// A *user-defined* `q` interpolator. The quasiquote diagnostic must not steal
// it: `q"..."` is only a quasiquote when nothing else in scope answers to it.
class MyQ(sc: StringContext) {
  def q(args: Any*): String = "user-q:" + sc.parts.mkString("|")
  def tq(args: Any*): String = "user-tq:" + sc.parts.mkString("|")
}

class Extractor {
  def apply(name: String): String = "<" + name + ">"
  def apply(n: Int): String = "<" + n + ">"
}

trait Universe {
  def Ident: Extractor
  def Literal: Extractor
  val tag: String
}

object SmallUniverse extends Universe {
  def Ident: Extractor = new Extractor
  def Literal: Extractor = new Extractor
  val tag: String = "small"
}

object Main {
  implicit def toMyQ(sc: StringContext): MyQ = new MyQ(sc)

  val u: Universe = SmallUniverse
  import u._

  def show(parts: Seq[String], args: Seq[Any]): String = {
    val sb = new StringBuilder
    var i = 0
    while (i < parts.length) {
      sb.append(parts(i))
      if (i < args.length) sb.append("$" + args(i))
      i += 1
    }
    sb.toString
  }

  def main(argv: Array[String]): Unit = {
    // A package-object `val` and a package-object `def`, both from the jar.
    println(scala.math.Pi)
    println(scala.math.abs(-7))
    println(scala.math.max(3, 9))

    // Parameterless defs used as functions: `Literal(1)` is
    // `Literal.apply(1)`, and the overload is picked on the result's members.
    println(Literal(1))
    println(Ident("x"))
    println(tag)

    // The same through the value's own path, not the import.
    println(u.Literal("via-path"))

    // What a quasiquote is made of, spelled out: literal parts and holes.
    println(show(Seq("a", "b", "c"), Seq(1, 2)))

    // `q` here is the user's own interpolator, not a quasiquote.
    val n = 1
    println(q"a${n}b")
    println(tq"c")
  }
}
