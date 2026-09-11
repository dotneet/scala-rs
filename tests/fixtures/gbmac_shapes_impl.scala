// Expansions built of the tree shapes slick's `mapToImpl` returns, without
// slick: a pattern-matching anonymous function (`Match` with an empty
// selector) whose cases use alternatives, guards, binders, sequence
// wildcards, extractors and typed patterns with wildcard type arguments; a
// local class with an overriding member that calls `super`; named
// arguments; an existential type in a cast. `docs/macros.md` §7.25.
//
// Compiled by real scalac, since only nsc writes the `@macroImpl` binding
// scala-rs reads back; `gbmac_shapes_use.scala` is compiled by both
// compilers against it (`crates/cli/tests/gbmac.rs`).
package gbmacs

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

// The superclass the expansion's local class extends: a constructor argument
// in the parents clause and a `super` call in an overriding member, the two
// things slick's fast-path converter does.
abstract class ShapesBase(val prefix: String) {
  def describe: String = prefix + ":base"
}

object Shapes {
  def classify: Any => String = macro classifyImpl
  def named(n: Int): String = macro namedImpl
  def sizeOf(x: Any): Int = macro sizeOfImpl

  def classifyImpl(c: Context): c.Tree = {
    import c.universe._
    q"""({
      case 0 | 1 => "small"
      case s: String if s.nonEmpty => "string " + s
      case l @ List(1, rest @ _*) => "list from one, rest " + rest.size + " of " + l.size
      case m: Map[_, _] => "map " + m.size
      case (a: Int, _) => "pair " + a
      case other => "other " + other
    }: (Any => String))"""
  }

  def namedImpl(c: Context)(n: c.Tree): c.Tree = {
    import c.universe._
    q"""{
      final class Sub(tag: String) extends _root_.gbmacs.ShapesBase(tag + "!") {
        override def describe = tag + "/" + super.describe
      }
      _root_.scala.collection.immutable.List.fill(n = $n)(elem = new Sub(tag = "t").describe).mkString(",")
    }"""
  }

  def sizeOfImpl(c: Context)(x: c.Tree): c.Tree = {
    import c.universe._
    q"($x: Any).asInstanceOf[List[_]].size"
  }
}
