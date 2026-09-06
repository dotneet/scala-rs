class VSqlGenericDefault[T](val value: T = 42)
class VSqlNumericBase(val value: Long = 42L)
class VSqlCurriedBase(val prefix: String)(val value: Int = 42)
class VSqlOverloadedBase(val x: String, val y: Int = 7) {
  def this(x: Int, y: Int) = this(x.toString, y)
}
class VSqlAuxDefaultBase(x: Int) {
  def this(s: String = "ok") = this(s.length)
  def value: Int = x
}
class VSqlExactOverloadBase(val x: String, val y: Int = 7) {
  def this(y: Int) = this("exact", y)
}

object VSqlNested {
  class Base[T](val value: T = 42)
}
