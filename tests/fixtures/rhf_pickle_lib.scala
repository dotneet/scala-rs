// What real scalac has to be able to read back out of our `ScalaSignature`.
//
// Four things were missing, and each one made a whole API invisible:
//
//  * the *arguments* of an applied type member -- `type Type[+A] <: Base with
//    Tag` came back as a proper type, so cats' no-boxing newtypes
//    (`NonEmptyChain`, `NonEmptySet`) reported "value head is not a member of
//    Type" for every operation;
//  * the *declaring class* of a type member this pickle does not contain, which
//    used to be minted fresh and root-owned: `found: Type[Int] (in
//    cats.data.NonEmptyChainImpl); required: Type[Int] (in <none>)`;
//  * the object a type member is *seen through* when a trait declares it
//    (`NonEmptySetImpl.Type`, not `Newtype.this.Type`), which is also what puts
//    the object's own ops conversion in the type's implicit scope;
//  * the `type` declarations of a *refinement*, which were dropped outright:
//    the `Aux` pattern (`def apply[F](implicit ev: R[F]): R.Aux[F, ev.Rep]`)
//    pickled as a bare `R[F]`.
package rhf

/** The newtype helper trait, mixed in by the object below: cats' `Newtype`. */
private[rhf] trait NT {
  private[rhf] type Base
  private[rhf] trait Tag extends Any
  type Type[A] <: Base with Tag
}

/** A newtype whose `Type` is *inherited*, so the prefix is the object. */
object SetImpl extends NT {
  private[rhf] def create[A](s: List[A]): Type[A] = s.asInstanceOf[Type[A]]
  private[rhf] def unwrap[A](s: Type[A]): List[A] = s.asInstanceOf[List[A]]

  def of[A](a: A, as: A*): Type[A] = create(a :: as.toList)

  implicit def ops[A](value: Type[A]): SetOps[A] = new SetOps(value)
}

sealed class SetOps[A](private[rhf] val value: SetImpl.Type[A]) {
  def head: A = SetImpl.unwrap(value).head
  def toList: List[A] = SetImpl.unwrap(value)
  def size: Int = toList.size
}

/** A newtype that declares `Type` itself, so the prefix is `this`. */
object ChainImpl {
  private[rhf] type Base
  private[rhf] trait Tag extends Any
  type Type[+A] <: Base with Tag

  private[rhf] def create[A](s: List[A]): Type[A] = s.asInstanceOf[Type[A]]
  private[rhf] def unwrap[A](s: Type[A]): List[A] = s.asInstanceOf[List[A]]

  def of[A](a: A, as: A*): Type[A] = create(a :: as.toList)

  implicit def ops[A](value: Type[A]): ChainOps[A] = new ChainOps(value)
}

sealed class ChainOps[A](private[rhf] val value: ChainImpl.Type[A]) {
  def head: A = ChainImpl.unwrap(value).head
  def toList: List[A] = ChainImpl.unwrap(value)
}

/** The `Aux` pattern: a refinement, and a result type that names a parameter. */
trait Rep[F] {
  type Repr
  def tabulate(f: Repr => Int): F
}

object Rep {
  type Aux[F, X] = Rep[F] { type Repr = X }

  implicit val forString: Rep.Aux[String, Boolean] = new Rep[String] {
    type Repr = Boolean
    def tabulate(f: Boolean => Int): String = f(true).toString + f(false).toString
  }

  def apply[F](implicit ev: Rep[F]): Rep.Aux[F, ev.Repr] = ev

  /** A refinement with a concrete right-hand side, and one with a bound. */
  def fixed: Rep[String] { type Repr = Boolean } = forString
  def bounded: Rep[String] { type Repr <: AnyVal } = forString
}

/** `Byte` and `Short` used to be pickled as root-owned references. */
object Bytes {
  def encode(value: Array[Byte]): String = new String(value, "UTF-8")
  def decode(s: String): Array[Byte] = s.getBytes("UTF-8")
  def bump(b: Byte): Byte = (b + 1).toByte
  def widen(s: Short): Short = (s + 1).toShort
}
