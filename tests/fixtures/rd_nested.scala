// Nested `object`s of the reflection API, and `<a val>.type` as a stable
// identifier. `docs/macros.md` §7.8 residuals 5 and 6, the two gaps §7.13.4
// names in front of a self-built `reify`.
//
// `trait Exprs { object Expr { … } }` compiles to an interface method
// `Expr()Lscala/reflect/api/Exprs$Expr$;` plus the module's own class file.
// `PickleSupply::complete_named` read only `Def` and `Val` members, so both
// spellings below -- through the path and through the wildcard import --
// were untrue diagnostics ("value Expr is not a member of Universe",
// "not found: value Expr").
//
// `scala.reflect.api.Mirror[scala.reflect.runtime.universe.type]` is the
// other half: `universe` is a `val`, so the path is stable, but a `val` read
// from a pickle arrives as a zero-argument *method* (a class file cannot tell
// the two apart) and `term_path_sym` did not accept one -- "stable identifier
// required, but scala.reflect.runtime.universe found".
//
// Real scalac 2.13.16 compiles and runs this file too; the two outputs must
// match line for line, which is what says the trees and the receivers are the
// same. A wrong receiver for a member `object` compiles and only fails at run
// time (`ClassCastException: Main$ cannot be cast to
// scala.reflect.api.Liftables`), so running it is the only test that catches
// it.
import scala.reflect.runtime.universe
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    // `<a pickled val>.type` in a type argument.
    val m: scala.reflect.api.Mirror[scala.reflect.runtime.universe.type] =
      universe.rootMirror
        .asInstanceOf[scala.reflect.api.Mirror[scala.reflect.runtime.universe.type]]
    println(m.staticClass("scala.Int").fullName)

    // A nested object reached through the wildcard import, applied with an
    // explicit type argument.
    val li = Liftable[Int]((n: Int) => Literal(Constant(n)))
    println(showRaw(li(5)))

    // The same object reached through the path.
    val li2 = universe.Liftable[String]((s: String) => Literal(Constant(s)))
    println(showRaw(li2("hi")))

    // `Expr` itself, both ways. It is the one the reify expansion needs.
    println(universe.Expr.getClass.getName.endsWith("Expr$"))
    println(Expr.getClass.getName.endsWith("Expr$"))
  }
}
