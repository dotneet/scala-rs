trait Query[+T] {
  def value: T
}

class TableQuery[T](val value: T) extends Query[T]

class TableQueryExtensionMethods[T](val q: Query[T] with TableQuery[T]) {
  def schema: String = q.value.toString
}

trait API {
  implicit def tableQueryToTableQueryExtensionMethods[T](
      q: Query[T] with TableQuery[T]
  ): TableQueryExtensionMethods[T] = new TableQueryExtensionMethods[T](q)
}
