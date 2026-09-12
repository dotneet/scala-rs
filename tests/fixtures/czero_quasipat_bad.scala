// Quasiquote-pattern body shapes `crates/typer/src/quasi_pattern.rs` does not
// deconstruct. Each one is reported, naming the shape. Silently accepting one
// would be worse than refusing it: the pattern would compile and then match
// the wrong trees, or none at all, at run time inside a macro.
//
// Real scalac 2.13.16 accepts all but the last (`...$` cannot be extracted
// with at all); these are refusals, never wrong acceptances.
object Main {
  val u = scala.reflect.runtime.universe
  import u._

  def mixedArgs(t: Tree): String = t match {
    case q"$f(..$as, y)" => "mixed"
    case _               => "no"
  }
  def writtenParam(t: Tree): String = t match {
    case q"(x: Int) => $r" => "written param"
    case _                 => "no"
  }
  def rightAssoc(t: Tree): String = t match {
    case q"$a :: $b" => "rassoc"
    case _           => "no"
  }
  def newC(t: Tree): String = t match {
    case q"new C($x)" => "new"
    case _            => "no"
  }
  def cond(t: Tree): String = t match {
    case q"if ($x) $y else $z" => "if"
    case _                     => "no"
  }
  def empty(t: Tree): String = t match {
    case q"" => "empty"
    case _   => "no"
  }
  def rank2(t: Tree): String = t match {
    case q"...$xss" => "rank2"
    case _          => "no"
  }
  def typeQuasi(t: Tree): String = t match {
    case tq"List[$a]" => "tq"
    case _            => "no"
  }
  // The dual of `Liftable`: nsc unlifts the matched tree through an
  // `Unliftable[C]` and reports "Can't find
  // reflect.runtime.universe.Unliftable[C], consider providing it" when there
  // is none (`test/files/neg/quasiquotes-unliftable-not-found`). Unlifting is
  // not implemented, so the ascription is refused rather than taken for an
  // ordinary typed pattern -- which would have accepted the program and then
  // bound a `Tree` to a `C`.
  class C
  def unlift(t: Tree): String = t match {
    case q"${c: C}" => "unlifted " + c.toString
    case _          => "no"
  }
}
