// The shape the bare-name fallback in `tree_to_type`'s `TreeKind::Select` arm
// exists for, at scala-library scale, and running.
//
// A `type` alias a jar class declares leaves no trace in the bytecode, so
// `lookup_qualified_type` -- which can only see symbols -- finds nothing for
// `Predef.Map`, and the arm falls through to `qualified_pickled_type_member`.
// Twirl writes `HtmlFormat.Appendable` in the parents clause of every
// generated template and in the signature of its `apply` on the very next
// line; `scala.Predef`'s aliases are the same construction out of a jar this
// repository always has.
//
// `qualifier_names_nothing` narrows the fallback to qualifiers that denote
// something. `Predef` denotes a module in a jar, so every prefix here must go
// on resolving -- and the results are executed, not merely compiled, because
// a prefix that resolved to the wrong alias would still have compiled.
//
// Requires `--scala-library`. Real scalac 2.13.16 prints the same lines; see
// `crates/cli/tests/qualfallback.rs`.

trait Sink[A] {
  def take(a: A): String
}

// The alias as a type *argument* in a parents clause -- Twirl's
// `BaseScalaTemplate[HtmlFormat.Appendable, ...]` -- and then in the ordinary
// signature right below it, which is the pairing that used to disagree.
object MapSink extends Sink[Predef.Map[Int, String]] {
  def take(a: Predef.Map[Int, String]): String = a.toString
}

// The same alias reached from the root.
object RootSink extends Sink[_root_.scala.Predef.Map[Int, String]] {
  def take(a: _root_.scala.Predef.Map[Int, String]): String = a.toString
}

// A nullary alias, which has no symbol of its own at all.
object StringSink extends Sink[Predef.String] {
  def take(a: Predef.String): String = a
}

// An alias on the `scala` package object rather than on `Predef`.
object ListSink extends Sink[scala.List[Int]] {
  def take(a: scala.List[Int]): String = a.mkString(",")
}

object Main {
  def widen(m: Predef.Map[Int, String]): Predef.Set[Int] = m.keySet

  def main(args: Array[String]): Unit = {
    val m: Predef.Map[Int, String] = Predef.Map(1 -> "one", 2 -> "two")
    println(MapSink.take(m))
    println(RootSink.take(m))
    println(StringSink.take("plain"))
    println(ListSink.take(List(3, 4, 5)))
    println(widen(m).toList.sorted.mkString("|"))
  }
}
