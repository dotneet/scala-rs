package compiledqueryorder

// Compile this source before Shape: its Packed alias is still lazy when the
// return type is read, but complete by the time the constructor is typed.
final class Parameters[PU, PP](pShape: Shape[ColumnsShapeLevel, PU, PU, ?])

object Parameters {
  def apply[U](implicit pShape: Shape[ColumnsShapeLevel, U, U, ?]): Parameters[U, pShape.Packed] =
    new Parameters[U, pShape.Packed](pShape)
}

object Main {
  def main(args: Array[String]): Unit =
    println(Class.forName("compiledqueryorder.Parameters").getSimpleName)
}
