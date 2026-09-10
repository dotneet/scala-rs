import scala.reflect.runtime.universe._
object Main {
  def main(args: Array[String]): Unit = {
    for (tpe <- List(typeOf[ConstructorVal], typeOf[ConstructorVar], typeOf[BodyVal], typeOf[BodyVar])) {
      val field = tpe.decl(TermName("x "))
      val getter = tpe.decl(TermName("x"))
      assert(field != NoSymbol && !field.isMethod && field.isPrivate)
      assert(field.info =:= typeOf[Int])
      assert(!field.isImplicit && !field.isParameter)
      assert(!getter.isParameter)
      assert(getter.asMethod.isGetter)
      assert(!(getter.info =:= typeOf[Int]))
      assert(field.asTerm.isParamAccessor == getter.asTerm.isParamAccessor)
      val setter = tpe.decl(TermName("x_$eq"))
      if (setter != NoSymbol)
        assert(setter.asTerm.isParamAccessor == getter.asTerm.isParamAccessor)
      println(tpe.members.exists(_.info =:= typeOf[Int]))
    }
    for (tpe <- List(typeOf[ConstructorVal], typeOf[ConstructorVar]);
         param <- tpe.typeSymbol.asClass.primaryConstructor.asMethod.paramLists.flatten) {
      assert(param.isParameter && !param.asTerm.isVar && !param.asTerm.isParamAccessor)
    }
    assert(typeOf[AbstractField].decl(TermName("x ")) == NoSymbol)
    val implicitField = typeOf[ImplicitField].decl(TermName("x "))
    val implicitGetter = typeOf[ImplicitField].decl(TermName("x"))
    assert(!implicitField.isImplicit && implicitGetter.isImplicit)
    assert(implicitField.isFinal && implicitGetter.isFinal)
    println(FieldMacros.noInt[StringField])
    println(FieldMacros.noInt[AbstractField])
  }
}
