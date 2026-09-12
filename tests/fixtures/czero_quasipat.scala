// Quasiquotes in *pattern* position, against the runtime universe so the
// match really runs. `crates/typer/src/quasi_pattern.rs` deconstructs them the
// way nsc's `UnapplyReifier` does; what this fixture pins is that the answers
// -- which trees match, which do not, and what each hole binds -- are the same
// as real scalac's, in both directions.
//
// The shape the cats slice needed is `fn` below:
// `case q"($param) => $trans[..$typeArgs]($arg)"`, which is how
// `core/src/main/scala-2/cats/arrow/FunctionKMacros.scala` recognises the
// function it lifts to a `FunctionK`.
object Main {
  val u = scala.reflect.runtime.universe
  import u._

  def lit(t: Tree): String = t match {
    case q"0" => "zero"
    case q"1" => "one"
    case _    => "other"
  }

  def fn(t: Tree): String = t match {
    case q"($param) => $trans[..$typeArgs]($arg)" =>
      s"fn param=$param trans=$trans typeArgs=$typeArgs arg=$arg"
    case _ => "no fn"
  }

  def app(t: Tree): String = t match {
    case q"$f($a)" => s"app f=$f a=$a"
    case _         => "no app"
  }

  def named(t: Tree): String = t match {
    case q"foo($a)" => s"foo($a)"
    case q"a.b($a)" => s"a.b($a)"
    case _          => "no named"
  }

  def spread(t: Tree): String = t match {
    case q"$f(..$as)" => s"spread f=$f as=$as len=${as.length}"
    case _            => "no spread"
  }

  def ops(t: Tree): String = t match {
    case q"$a + $b"    => s"plus($a,$b)"
    case q"$a.foo($b)" => s"foo($a,$b)"
    case q"$a.foo"     => s"sel($a)"
    case _             => "no ops"
  }

  def block(t: Tree): String = t match {
    case q"{ $a; $b }" => s"block2($a,$b)"
    case _             => "no block"
  }

  def stats(t: Tree): String = t match {
    case q"{ ..$ss }" => s"stats=$ss len=${ss.length}"
    case _            => "no stats"
  }

  def targs(t: Tree): String = t match {
    case q"$f[Int]($a)" => s"int($f,$a)"
    case q"$f[A]($a)"   => s"aa($f,$a)"
    case _              => "no targs"
  }

  // A rank-0 hole standing for the whole body matches any tree and binds it
  // *as a tree*: `x.children` only compiles because the hole is typed by the
  // universe's `Tree` and not by the scrutinee. Spliced in bare it would have
  // been an ordinary variable pattern, and `case q"$x"` would have matched an
  // `Option[Int]`.
  def whole(t: Tree): String = t match {
    case q"$x" => s"whole=$x kids=${x.children.length}"
  }

  def fixedArg(t: Tree): String = t match {
    case q"$f(1, $b)" => s"one_then($f,$b)"
    case _            => "no fixedArg"
  }

  def main(args: Array[String]): Unit = {
    val trees: List[Tree] = List(
      q"0",
      q"1",
      q"2",
      q"(x: Int) => g(x)",
      q"(x: Int) => g[Int](x)",
      q"foo(3)",
      q"a.b(4)",
      q"h(1, 2, 3)",
      q"h()",
      q"k",
      q"x + y",
      q"x.+(y)",
      q"x.foo(y)",
      q"x.foo",
      q"{ val a = 1; a }",
      q"{ p; r }",
      q"f[Int](3)",
      q"f[A](3)",
      q"f[Long](3)",
      q"f(3)",
      q"g(1, 7)",
      q"g(2, 7)"
    )
    for (t <- trees) {
      println("--- " + showRaw(t))
      println("  lit:      " + lit(t))
      println("  fn:       " + fn(t))
      println("  app:      " + app(t))
      println("  named:    " + named(t))
      println("  spread:   " + spread(t))
      println("  ops:      " + ops(t))
      println("  block:    " + block(t))
      println("  stats:    " + stats(t))
      println("  targs:    " + targs(t))
      println("  whole:    " + whole(t))
      println("  fixedArg: " + fixedArg(t))
    }
  }
}
