class VSqlGenericDefault[T](val value: T = 42)
class VSqlNumericBase(val value: Long = 42L)
class VSqlCurriedBase(val prefix: String)(val value: Int = 42)
class VSqlOverloadedBase(val x: String, val y: Int = 7) {
  def this(x: Int, y: Int) = this(x.toString, y)
}

object VSqlNested {
  class Base[T](val value: T = 42)
}
