// Macro implementations whose `c.typecheck` asks scala-rs something it
// genuinely cannot answer. `docs/macros.md` §7.20.
//
// Four of the six call sites in `mtc_bad.scala` compile under real scalac
// 2.13.16 and print `Int(1) / Int(1) / Int(1) / caught` there; the point
// of the fixture is that scala-rs **refuses each of them with a reason that
// names what was missing**, rather than answering approximately. A
// `c.typecheck` that guessed would have the implementation build its expansion
// out of a type or a tree the compiler never actually agreed to.
//
// The other two -- `patternMode` and `unbuildable` -- real scalac rejects as
// well, for reasons of its own (`package scala is not a value`, and
// `unexpected tree: Trees$Star`). They are here because scala-rs has to refuse
// them *by name* rather than reading PATTERNmode as TERMmode or dropping a
// node it cannot rebuild.
import scala.reflect.macros.blackbox.Context

object MtcBadImpl {
  // A typer mode scala-rs has no switch for. nsc really can run its typer
  // with implicit views turned off; ignoring the flag would answer a
  // different question from the one that was asked.
  def noViewsImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Literal(Constant(1)), c.TERMmode, WildcardType, false, true, false)
    Literal(Constant(t.tpe.toString))
  }

  // Likewise for macro expansion inside the typechecked tree.
  def noMacrosImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Literal(Constant(1)), c.TERMmode, WildcardType, false, false, true)
    Literal(Constant(t.tpe.toString))
  }

  // An expected type. scala-rs types the tree with no expectation, and a `pt`
  // changes what the answer is allowed to be.
  def withPtImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Literal(Constant(1)), c.TERMmode, typeOf[Any], false, false, false)
    Literal(Constant(t.tpe.toString))
  }

  // PATTERNmode. The mode reaches scala-rs, which says so by name rather than
  // reading it as TERMmode.
  def patternModeImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Ident(TermName("scala")), c.PATTERNmode, WildcardType,
                        false, false, false)
    Literal(Constant(t.tpe.toString))
  }

  // A non-silent typecheck that fails, and an implementation that catches the
  // failure. nsc hands it a `TypecheckException`; this bridge cannot, because
  // the `Context` is a `java.lang.reflect.Proxy` and a proxy wraps a checked
  // exception the interface does not declare. Whatever the implementation did
  // next was decided on something it was not told, so the expansion is
  // refused rather than accepted.
  def caughtFailureImpl(c: Context)(): c.Tree = {
    import c.universe._
    val text =
      try {
        c.typecheck(Ident(TermName("noSuchNameAnywhere"))).tpe.toString
      } catch {
        case ex: Throwable => "caught"
      }
    Literal(Constant(text))
  }

  // A tree shape the engine hands over that scala-rs cannot rebuild. `Star`
  // is a pattern node; there is nothing in scala-rs's AST that stands for it
  // in an expression, so it is refused by name.
  def unbuildableImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Star(Ident(TermName("x"))), c.TERMmode, WildcardType,
                        false, false, false)
    Literal(Constant(t.tpe.toString))
  }
}
