import scala.language.experimental.macros
import scala.reflect.macros.whitebox

object ConstructorReflection {
  def inspect[T]: String = macro ConstructorReflectionMacros.inspect[T]
}

class ConstructorReflectionMacros(val c: whitebox.Context) {
  import c.universe._

  private def describe(owner: Type, method: MethodSymbol): String =
    method.paramLists
      .map { params =>
        val flags = params.map { param =>
          val member = owner.decl(TermName(param.name.toString))
          val isAccessor = member != NoSymbol && member.isTerm && member.asTerm.isCaseAccessor
          s"${param.isImplicit}:$isAccessor"
        }
        s"${params.size}[${flags.mkString(",")}]"
      }
      .mkString("/")

  private def describeCopy(method: MethodSymbol): String =
    method.paramLists
      .map { params =>
        val flags = params.map { param =>
          val tpe = param.typeSignature
          val rendered = if (tpe == NoType) "<none>" else compactType(tpe)
          s"${param.isImplicit}:$rendered:${tpe != NoType}"
        }
        s"${params.size}[${flags.mkString(",")}]"
      }
      .mkString("/")

  private def compactType(tpe: Type): String = {
    val name = tpe.typeSymbol.name.toString
    if (tpe.typeArgs.isEmpty) name
    else s"$name[${tpe.typeArgs.map(compactType).mkString(",")}]"
  }

  def inspect[T: c.WeakTypeTag]: c.Expr[String] = {
    val tpe = weakTypeOf[T]
    val classSymbol = tpe.typeSymbol.asClass
    val constructor = classSymbol.primaryConstructor.asMethod
    val companion = classSymbol.companionSymbol
    val apply = companion.typeSignature.member(TermName("apply")).alternatives.collect {
      case method: MethodSymbol => method
    }.head
    val copy = classSymbol.toType.member(TermName("copy")).alternatives.collect {
      case method: MethodSymbol => method
    }.head
    val result =
      s"ctor=${describe(tpe, constructor)};apply=${describe(tpe, apply)};copy=${describeCopy(copy)}"
    c.Expr[String](Literal(Constant(result)))
  }
}
