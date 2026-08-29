// A parent constructor whose implicit clause has no witness must be an error,
// not a silently empty `TypedRep.<init>()` call that dies at run time.
//
// scalac 2.13.16:
//   pimpl_bad.scala:11: error: could not find implicit value for parameter
//     tpe: TT[String]
//   class NoEvidence extends TypedRep[String]
//                            ^
trait TT[T]
class TypedRep[T](implicit val tpe: TT[T])
class NoEvidence extends TypedRep[String]

object Main {
  def main(args: Array[String]): Unit = println(new NoEvidence())
}
