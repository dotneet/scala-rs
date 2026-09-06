class VSqlGenericDefault[T](val value: T = 42)
class VSqlNumericBase(val value: Long = 42L)
class VSqlCurriedBase(val prefix: String)(val value: Int = 42)

object VSqlNested {
  class Base[T](val value: T = 42)
}
