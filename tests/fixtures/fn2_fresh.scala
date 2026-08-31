// The three quasiquote forms whose expansion nsc builds out of a *fresh name*:
// a `_` placeholder function literal, a `_` type argument (an existential),
// and a right-associative operator. `docs/macros.md` §7.10.
//
// What makes them different from every other form is that nsc's expansion is
// not one expression but a **block**: it emits
// `val nn$macro$k = u.internal.reificationSupport.freshTermName("x$")` ahead of
// the call and uses `nn$macro$k` wherever the name is needed, so the name in
// the reflect `Tree` is drawn from the universe's counter at run time. The
// shapes were read off real scalac 2.13.16 with `-Ymacro-debug-lite`.
//
// Every line is compared with real scalac, which prints
// `expected/fn2_fresh.txt`. The fresh names are renumbered per line before the
// comparison -- see `fn2_fresh_matches_real_scalac` in
// `crates/cli/tests/quasi.rs` -- because the universe's counter is global and
// nsc happens to draw its names right-to-left; what the comparison keeps is
// the tree's shape and *which binder each occurrence refers to*.
//
// Needs scala-reflect.jar on the classpath.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    val x: Tree = q"x"
    val t: Tree = tq"Int"
    val n: TermName = TermName("nm")

    // --- `_` placeholder function literals -------------------------------
    println(showRaw(q"_.get"))
    println(showRaw(q"_ + 1"))
    println(showRaw(q"f(_)"))
    println(showRaw(q"(_: Int).get"))
    println(showRaw(q"_.foo(_)"))
    println(showRaw(q"_.get.map(_.x)"))
    println(showRaw(q"{ val v = _.get; v }"))
    println(showRaw(q"(f _).andThen(_.get)"))
    // slick's `ShapedValue.mapToImpl`, spelled out.
    println(showRaw(q"(($x.unapply _) : $t => Option[$t]).andThen(_.get)"))
    // A written parameter is still a written parameter.
    println(showRaw(q"(y: Int) => y.get"))

    // --- `_` type arguments: existentials --------------------------------
    println(showRaw(tq"P[_]"))
    println(showRaw(tq"P[_, _]"))
    println(showRaw(tq"P[$t, _]"))
    println(showRaw(tq"P[_ <: Int]"))
    println(showRaw(tq"P[_ >: Null <: Int]"))
    println(showRaw(tq"Option[P[_]]"))
    println(showRaw(tq"P[_] => Q[_]"))
    println(showRaw(q"y.asInstanceOf[P[_, _]]"))
    println(showRaw(q"new C[P[_]](1)"))
    // In a *pattern* a bare `_` type argument is a type-variable pattern and
    // binds nothing; with bounds it is an existential there too.
    println(showRaw(pq"_: R[_, _]"))
    println(showRaw(pq"_: R[_ <: Int]"))
    // slick's `ProductResultConverter[_, _, _, _]`, in the position it stands.
    println(showRaw(q"{ case tm @ P(_: R[_, _], _, _) => tm }"))

    // --- right-associative operators -------------------------------------
    println(showRaw(q"a :: b"))
    println(showRaw(q"$x :: $x"))
    println(showRaw(q"a :: b :: c"))
    println(showRaw(q"a +: b"))
    println(showRaw(q"v.$n :: $x"))
    // Written as a call it is an ordinary selection, and nsc builds no block.
    println(showRaw(q"b.::(a)"))
    // A pattern needs no block: there is nothing to evaluate.
    println(showRaw(q"{ case a :: b => a }"))
    // Left-associative neighbours are untouched.
    println(showRaw(q"a :+ b"))

    // --- all three at once, in one quasiquote ----------------------------
    println(showRaw(q"(g _).andThen(_.asInstanceOf[P[_, _]] :: $x)"))

    // --- slick's `ShapedValue.mapToImpl`, the shape that was blocked -----
    // Both spellings of `_` in one quasiquote: a type-variable pattern in the
    // `case`, and an existential in the `asInstanceOf`.
    val xs: List[Tree] = List(q"val a = 1")
    println(showRaw(q"""
      val fpMatch: (_root_.scala.Any => _root_.scala.Any) = {
        case tm @ _root_.p.TypeMappingResultConverter(_: _root_.p.ProductResultConverter[_, _, _, _], _, _) =>
          new _root_.p.SimpleFastPathResultConverter[_root_.scala.Any, _root_.scala.Any, _root_.scala.Any, $t](tm.asInstanceOf[_root_.p.TypeMappingResultConverter[_root_.scala.Any, _root_.scala.Any, _root_.scala.Any, $t, _]]) {
            ..$xs
          }
        case tm => tm
      }
    """))
  }
}
