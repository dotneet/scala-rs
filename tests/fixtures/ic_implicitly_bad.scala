// Coercing `Predef.identity` / `locally` / `implicitly`'s erased result only
// ever adds a cast to a call the typer already accepted. It must not make one
// it rejects go through. Real scalac 2.13.16 rejects all three of these too.
class Cell[A](val tag: String)

object Bad {
  implicit val cs: Cell[String] = new Cell[String]("s")

  // No witness at this type.
  val missing: Cell[Boolean] = implicitly[Cell[Boolean]]

  // `identity` is `(A)A`, not a conversion.
  val wrong: Int = identity("not an int")

  // The result still has the type the argument had; a cast on the way out
  // does not widen what may be selected from it.
  val noSuchMember: String = locally(new Cell[String]("s")).show
}
