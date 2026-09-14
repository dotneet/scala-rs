// A generic case class with a second parameter list exercises both metadata
// writers: constructor/apply Generic Signatures and MethodParameters names.
case class GenericMeta[A](value: A)(extra: List[A])
final case class GenericValue[A](value: A) extends AnyVal

final class GenericOuter[A](val prefix: String) {
  case class Nested(value: A)
  def nested(value: A): Nested = Nested(value)

  def captured(value: A): AnyRef = {
    val suffix = "!"
    class Captured(val item: A) {
      override def toString: String = prefix + item + suffix
    }
    new Captured(value)
  }
}

object Main {
  def names(parameters: Array[java.lang.reflect.Parameter]): String = {
    var result = ""
    var i = 0
    while (i < parameters.length) {
      if (i != 0) result += ","
      result += parameters(i).getName
      i += 1
    }
    result
  }

  def main(args: Array[String]): Unit = {
    val ctor = classOf[GenericMeta[Any]].getDeclaredConstructors()(0)
    val methods = GenericMeta.getClass.getDeclaredMethods
    var i = 0
    var apply: java.lang.reflect.Method = null
    while (i < methods.length && apply == null) {
      val candidate = methods(i)
      if (candidate.getName == "apply" && candidate.getParameterCount == 2)
        apply = candidate
      i += 1
    }
    println(names(ctor.getParameters))
    println(names(apply.getParameters))
    println(ctor.toGenericString)
    println(apply.toGenericString)

    val value = new GenericMeta[String]("value")(List("extra"))
    println(value.productArity)
    println(value.toString)

    val outer = new GenericOuter[String]("nested:")
    println(outer.nested("value"))
    println(outer.captured("value"))
  }
}
