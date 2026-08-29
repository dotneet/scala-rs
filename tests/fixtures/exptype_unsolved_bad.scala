// Neither the arguments nor the expected type pin `T` down, so the implicit
// clause is searched with `T` still open. nsc:
//   error: could not find implicit value for parameter tt: TypedType[T]
class Rep[T](val name: String)

trait TypedType[T] { def label: String }

object Library {
  def column[T](name: String)(implicit tt: TypedType[T]): Rep[T] =
    new Rep[T](name + ":" + tt.label)
}

object Main {
  def main(args: Array[String]): Unit = {
    val r = Library.column("id")
    println(r.name)
  }
}
