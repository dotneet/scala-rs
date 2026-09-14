// A compact source-only version of Slick's OptionMapper2 overload shape.
//
// The argument to `Ops.===` is declared as `Rep[P2]`, while the actual
// argument is an `Option[A]` that must first use `valueToRep`.  The implicit
// OptionMapper2 then determines the result type.  This is the path used by
// Slick's `OptionColumnExtensionMethods.===` for both Some and None.
trait Rep[T]
trait TypedType[T]
trait BaseTypedType[T] extends TypedType[T]
trait OptionMapper[BR, R]
trait OptionMapper2[B1, B2, BR, P1, P2, R] extends OptionMapper[BR, R]

object OptionMapper2 {
  implicit def getTT[B1, B2: BaseTypedType, P2 <: B2, BR]:
      OptionMapper2[B1, B2, BR, B1, P2, BR] = null
  implicit def getTO[B1, B2: BaseTypedType, P2 <: B2, BR]:
      OptionMapper2[B1, B2, BR, B1, Option[P2], Option[BR]] = null
  implicit def getOT[B1, B2: BaseTypedType, P2 <: B2, BR]:
      OptionMapper2[B1, B2, BR, Option[B1], P2, Option[BR]] = null
  implicit def getOO[B1, B2: BaseTypedType, P2 <: B2, BR]:
      OptionMapper2[B1, B2, BR, Option[B1], Option[P2], Option[BR]] = null
}

trait Ops[B1, P1] {
  def ===[P2, R](e: Rep[P2])(
      implicit om: OptionMapper2[B1, B1, Boolean, P1, P2, R]
  ): Rep[R] = null
}

object Main {
  implicit val byteArrayType: BaseTypedType[Array[Byte]] = null
  implicit def optionType[T](implicit t: TypedType[T]): TypedType[Option[T]] = null
  implicit def valueToRep[T: TypedType](x: T): Rep[T] = null

  val column: Ops[Array[Byte], Option[Array[Byte]]] = null
  val some = column === (Some(Array[Byte](1, 2, 3)): Option[Array[Byte]])
  val none = column === (None: Option[Array[Byte]])
}
