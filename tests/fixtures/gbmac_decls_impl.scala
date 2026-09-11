// A macro that prints what its type argument's symbol answers: the class's
// flags, parents and declarations, and its companion's. The type argument is
// a class the *calling* run is compiling (`gbmac_decls_use.scala`), so under
// scala-rs every answer comes from the engine's mirror over the run's own
// symbols (`crates/typer/src/expand_mirror.rs`, `docs/macros.md` §7.25), and
// under real scalac from nsc's typer. The two outputs must be identical.
//
// The flag string is nsc's own `debugFlagString` minus the markers nsc's
// typer sets on itself while it works (`<triedcooking>` records that a name
// was looked up for Java interop, nothing a program declares).
package gbmac

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object Decls {
  def show[T]: String = macro showImpl[T]

  def showImpl[T: c.WeakTypeTag](c: blackbox.Context): c.Tree = {
    import c.universe._
    def fl(x: Symbol): String =
      x.asInstanceOf[scala.reflect.internal.Symbols#Symbol].debugFlagString
        .split(' ').filter(f => f.nonEmpty && f != "<triedcooking>").mkString(" ")
    def params(x: Symbol): String =
      if (!x.isMethod) ""
      else x.asMethod.paramLists.map(_.map { p =>
        p.name.toString + (if (p.asTerm.isParamWithDefault) "=" else "") + ":" + p.typeSignature
      }.mkString("(", ", ", ")")).mkString
    def decl(d: Symbol): String = {
      val kind =
        if (d.isMethod) "def" else if (d.isModule) "object" else if (d.isClass) "class"
        else if (d.isType) "type" else "val"
      lazy val t = d.asTerm
      val preds = List(
        "getter" -> (d.isTerm && t.isGetter), "setter" -> (d.isTerm && t.isSetter),
        "paramAcc" -> (d.isTerm && t.isParamAccessor), "caseAcc" -> (d.isTerm && t.isCaseAccessor),
        "stable" -> (d.isTerm && t.isStable), "isVal" -> (d.isTerm && t.isVal),
        "isVar" -> (d.isTerm && t.isVar), "lazy" -> (d.isTerm && t.isLazy),
        "ctor" -> d.isConstructor
      ).collect { case (n, true) => n }.mkString(",")
      s"  $kind ${d.name.toString.replace(" ", "<sp>")} [${fl(d)}] {$preds}${params(d)} :: ${d.typeSignature.resultType}\n"
    }
    val t = weakTypeOf[T]
    val s = t.typeSymbol
    val sb = new StringBuilder
    sb ++= s"${s.fullName} [${fl(s)}] case=${s.asClass.isCaseClass} tpe=$t\n"
    sb ++= s"  parents=${s.asClass.baseClasses.map(_.fullName).mkString(", ")}\n"
    sb ++= s"  primary=${params(s.asClass.primaryConstructor)}\n"
    for (d <- t.decls) sb ++= decl(d)
    val accessors = t.decls.collect { case m: MethodSymbol if m.isCaseAccessor => m.name.toString }
    sb ++= s"  caseAccessors=${accessors.mkString(",")}\n"
    val fields = t.decls.collect {
      case f: TermSymbol if f.isVal && f.isCaseAccessor => f.name.toString.trim + ":" + f.typeSignature
    }
    sb ++= s"  caseFields=${fields.mkString(",")}\n"
    val comp = s.companion
    if (comp == NoSymbol) sb ++= "  companion=<none>\n"
    else {
      sb ++= s"  companion=${comp.fullName} [${fl(comp)}] back=${comp.companion == s}\n"
      sb ++= s"  companion parents=${comp.info.baseClasses.map(_.fullName).mkString(", ")}\n"
      for (d <- comp.info.decls) sb ++= decl(d)
      sb ++= s"  tupled=${comp.info.member(TermName("tupled")) != NoSymbol}" +
        s" curried=${comp.info.member(TermName("curried")) != NoSymbol}\n"
    }
    Literal(Constant(sb.toString))
  }
}
