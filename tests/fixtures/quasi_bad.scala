// Quasiquotes must be diagnosed, never silently accepted.
//
// Two different gaps, told apart by `crates/typer/src/quasiquote.rs`:
//
//  * `q""` has no body to reify -- reported as `unimplemented syntax:
//    quasiquote q"..."`;
//  * `q"$x + 1"` is perfectly good Scala inside, and what is missing is the
//    reification step, which nsc does with a compiler-internal macro that has
//    no implementation in `scala-reflect.jar` (see `docs/macros.md` §6.2).
//
// Before this, both came out as `value q is not a member of StringContext`,
// which is wrong: `q` is a member of `Quasiquotes.Quasiquote`.
import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object QuasiBad {
  def impl(c: blackbox.Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    val empty = q""
    val term = q"$x + 1"
    val tpe = tq"_root_.scala.List[Int]"
    val pat = pq"_root_.scala.Some(y)"
    val cse = cq"y => y"
    c.Expr[Int](term)
  }
}
