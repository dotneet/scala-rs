// The library half of the `agent/gbopt` fixture. Compiled by **real scalac**
// and handed to scala-rs as a jar: all three defects are about what a class
// file and a pickle say about someone else's code, so a source-only fixture
// cannot show any of them.
package gboptlib

/** Root 1 -- the implicit scope's *candidates* were never warmed.
  *
  * The only witness for `Box[Option[T]]` is a companion `implicit def` whose
  * result type is a *subclass* of what is wanted, and `OptBox extends
  * Box[Option[T]]` is written only in `OptBox`'s own class file. Until
  * something reads it, the candidate fits nothing.
  *
  * slick's `TypedType.typedTypeToOptionTypedType[T]: OptionTypedType[T]` is
  * the real case, behind every `column[Option[String]]`.
  */
trait Box[T] { def name: String }
trait OptBox[T] extends Box[Option[T]]

object Box {
  implicit val strBox: Box[String] = new Box[String] { def name = "str" }
  implicit def optBox[T](implicit b: Box[T]): OptBox[T] =
    new OptBox[T] { def name = "opt(" + b.name + ")" }
}

/** Root 2 -- a mixin forwarder flattens the parameter clauses.
  *
  * `tagIn` is declared by the trait and inherited by the class, so scalac
  * writes a forwarder for it into `BaseOps`'s class file:
  * `tagIn(Iterable, Wit)Res`, one flat parameter list, because a class file
  * has no way to say where a clause ends. `BaseOps`'s own pickle does not
  * declare `tagIn` at all -- it is inherited -- so nothing replaced the
  * forwarder, and it shadowed the correctly clause-split declaration on
  * `Ops`.
  *
  * slick's `ColumnExtensionMethods#inSet` / `#in` / `#inSetBind`, reached
  * through `BaseColumnExtensionMethods`, are the real case.
  */
class Res[R](val text: String)
trait Wit[B, P, R] { def show(p: P, xs: Iterable[B]): Res[R] }
object Wit {
  implicit val intWit: Wit[Int, Int, Boolean] =
    (p: Int, xs: Iterable[Int]) => new Res[Boolean](p + "/" + xs.mkString(","))
}

trait Ops[B, P] {
  def self: P
  def tagIn[R](xs: Iterable[B])(implicit w: Wit[B, P, R]): Res[R] = w.show(self, xs)
}
final class BaseOps[P](val self: P) extends Ops[P, P]

object Entry {
  implicit def toBaseOps[P](p: P): BaseOps[P] = new BaseOps[P](p)
}

/** Root 3 -- a class file read once, but for the wrong owner.
  *
  * `later`'s signature names `scala.concurrent.ExecutionContext`, so reading
  * this object's class file enters that trait as a stub. An import prefix
  * then walks `scala.concurrent.ExecutionContext.Implicits` starting from
  * that *class* symbol, reads `ExecutionContext$Implicits$.class` under it,
  * and every later route is told the file is already loaded -- including the
  * one that asks the companion `ExecutionContext$`, which is where an import
  * selector looks.
  */
object Slickish {
  def later(n: Int)(implicit ec: scala.concurrent.ExecutionContext): Int = n + 1
}
