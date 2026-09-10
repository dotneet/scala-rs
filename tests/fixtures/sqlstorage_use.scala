import scala.reflect.runtime.universe._
object Main {
 def main(args: Array[String]): Unit = {
  for (name <- List("Hidden", "Plain", "Private", "Default", "Lazy", "Abstract", "Mutable", "HiddenBody", "HiddenLazy", "HiddenTrait")) {
   val t = runtimeMirror(getClass.getClassLoader).staticClass(name).toType
   val ds = t.decls.toList.filter(_.name.toString.trim == "x").sortBy(_.name.toString)
   println(name + ":" + ds.map(d => d.name.toString.replace(" ", "_") + ":" + d.info.toString + ":private=" + d.isPrivate + ":method=" + d.isMethod).mkString(","))
  }
  for (name <- List("Hidden", "Plain", "Private", "Default", "Mutable", "FinalParam", "ProtectedParam")) {
   val p = runtimeMirror(getClass.getClassLoader).staticClass(name).toType.decl(termNames.CONSTRUCTOR).asMethod.paramLists.head.head
   println(name + ":private=" + p.isPrivate + ":protected=" + p.isProtected + ":final=" + p.isFinal + ":implicit=" + p.isImplicit + ":parameter=" + p.isParameter)
  }
  val m = new Mutable(1); m.set(6)
  println(List(new Hidden(1).get, new Plain(2).get, new Private(3).get, new Default().x, m.get, new HiddenBody().get, new HiddenLazy().get, new ConcreteTrait().get).mkString(","))
  println(new GenericDefault[String]().xs.isEmpty)
  println(new DefaultsOwner.Nested().x)
  println(new SecondaryDefault().x)
  println(new EffectDefault().x + ":" + new EffectDefault().x + ":" + DefaultSource.calls)
  println(new CompanionDefault().x + ":" + CompanionDefault.label)
  println(new PrivateOverload(1).x("visible"))

 }
}
