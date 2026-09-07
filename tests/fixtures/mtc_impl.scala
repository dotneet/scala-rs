// Macro implementations that ask the *running compiler* a question in the
// middle of their own expansion: `c.typecheck`. `docs/macros.md` §7.20.
//
// This is the half of the bridge that runs backwards. Everything else the
// engine does it can do on its own, inside `scala.reflect.runtime.universe`,
// with the macro classpath to look things up on. `c.typecheck` cannot: what a
// tree means depends on the scope the macro was called from, which is a place
// only scala-rs has ever been. So the engine stops, writes the tree back over
// the pipe, and scala-rs types it -- in the real typer, at the real call site
// -- and answers.
//
// Compiled on its own so that `mtc_use.scala` can expand against it, the way
// nsc requires (`crates/cli/tests/macromirror.rs`).
import scala.reflect.macros.blackbox.Context

object MtcImpl {
  // A literal. nsc types it as the *constant* type `Int(1)`, not `Int`, and
  // so must this: the answer travels as a constant type rather than being
  // widened on the way.
  def constTypeImpl(c: Context)(): c.Tree = {
    import c.universe._
    Literal(Constant(c.typecheck(Literal(Constant(1))).tpe.toString))
  }

  // A name the *call site* binds and this file has never heard of. The
  // implementation cannot resolve it; the typer at the call site can, and
  // that is the whole point.
  def typeOfNameImpl(c: Context)(): c.Tree = {
    import c.universe._
    Literal(Constant(c.typecheck(Ident(TermName("secondsInAnHour"))).tpe.toString))
  }

  // The mirror over the current run's symbols. `origin` is declared to
  // return a class **this compilation is itself defining**, so it has no
  // class file and `mirror.staticClass` could never find it. The answer comes
  // from scala-rs, which is where that class exists.
  def runClassTypeImpl(c: Context)(): c.Tree = {
    import c.universe._
    Literal(Constant(c.typecheck(Ident(TermName("origin"))).tpe.toString))
  }

  // A member selected from a value of that same current-run class: the typer
  // has to look inside the class, not merely name it.
  def memberTypeImpl(c: Context)(): c.Tree = {
    import c.universe._
    val sel = Select(Ident(TermName("origin")), TermName("label"))
    Literal(Constant(c.typecheck(sel).tpe.toString))
  }

  // TYPEmode: the tree is read as a type rather than as an expression, and
  // the answer is a `TypeTree` carrying it.
  def typeModeImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Ident(TypeName("Marker")), c.TYPEmode)
    Literal(Constant(t.tpe.toString))
  }

  // A silent typecheck of something that does not typecheck. nsc answers
  // `EmptyTree` rather than raising, and an implementation reads the answer
  // off `isEmpty`; this is how a macro probes whether a name exists.
  def probeImpl(c: Context)(): c.Tree = {
    import c.universe._
    val missing = c.typecheck(Ident(TermName("noSuchNameAnywhere")), c.TERMmode,
                              WildcardType, true, false, false)
    val present = c.typecheck(Ident(TermName("secondsInAnHour")), c.TERMmode,
                              WildcardType, true, false, false)
    Literal(Constant(missing.isEmpty.toString + "," + present.isEmpty.toString))
  }

  // A class whose own description mentions itself. Describing it means
  // describing the type of `next`, which is the class being described -- so
  // the walk has to notice and stop. It stops by handing the engine the name
  // it has already created the symbol for, which is the same symbol, so the
  // description closes over itself the way nsc's does.
  def selfRefImpl(c: Context)(): c.Tree = {
    import c.universe._
    val t = c.typecheck(Ident(TermName("aNode")))
    Literal(Constant(t.tpe.toString + "/" + t.tpe.member(TermName("next")).info.toString))
  }

  // The tree the typer produced comes back, not merely its type: this one is
  // spliced straight into the expansion, so what runs is what `c.typecheck`
  // returned.
  def echoImpl(c: Context)(): c.Tree = {
    import c.universe._
    c.typecheck(Apply(Select(Ident(TermName("secondsInAnHour")), TermName("$plus")),
                      List(Literal(Constant(1)))))
  }
}
