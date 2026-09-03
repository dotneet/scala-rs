// Overload resolution roots from slick (`agent/ovl4`).
//
// Five things that had nothing to do with each other beyond the diagnostic
// they produced:
//
//  1. a rigid type parameter *argument* was scored applicable to every
//     parameter, so `String.valueOf(r)` was `ambiguous overload`;
//  2. a compound parameter mentioning the callee's own type parameter
//     (`ScalaType[A] with BaseTypedType[A]`) matched nothing;
//  3. a monomorphic callee handed its argument no expected type, so an
//     argument whose own type parameters are inferred settled them from
//     itself (`takeBox(Box(n))`);
//  4. a rigid type parameter argument now goes through its *upper bound*
//     (`fetch: F <: Option[String]` to an `Option[A]` parameter);
//  5. constructors are not inherited -- `java.util.Properties`'s
//     alternatives had `Hashtable`'s among them.
import java.io.OutputStreamWriter
import java.io.PrintWriter

trait Dumpable
class NodeX extends Dumpable { override def toString = "NodeX" }

final case class RefId[E <: AnyRef](e: E)

trait ScalaType[T] { def label: String }
trait BaseTypedType[T]

class Comprehension[+F <: Option[String]](val fetch: F)

case class Box[E](e: E)

object Main {
  type BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]
  type ColumnType[T] = ScalaType[T]

  // (1) `value` is a bare `R`; only `valueOf(Object)` takes it.
  case class SuccessAction[+R](value: R) {
    def info: String = String.valueOf(value)
  }

  // (2) the parameter is the compound alias written in the callee's own `A`.
  def describe[A](t: BaseColumnType[A]): String = t.label
  // ... and the same compound argument against a plain `ScalaType[U]`.
  class Mapped[T, U](val baseType: ColumnType[U]) {
    def label: String = baseType.label
  }
  def base[U](implicit ev: BaseColumnType[U]): String =
    describe(ev) + "/" + new Mapped[Int, U](ev).label

  implicit val strType: BaseColumnType[String] =
    new ScalaType[String] with BaseTypedType[String] { def label = "str" }

  // (3) `Box[E]` is invariant, so only the parameter's type can say `E = Any`.
  def takeBox(b: Box[Any]): String = "box:" + b.e
  def takeRefId(r: RefId[Dumpable]): String = "refid:" + r.e

  // (4) `F` is only what `Option[String]` is.
  def mapOrNone[A](o: Option[A])(f: A => A): Option[A] = o.map(f)
  def fetched[F <: Option[String]](c: Comprehension[F]): Option[String] =
    mapOrNone(c.fetch)(_.toUpperCase).orElse(c.fetch)

  def main(args: Array[String]): Unit = {
    println(SuccessAction("x").info)
    println(SuccessAction(42).info)
    println(base[String])
    println(takeBox(Box("boxed")))
    println(takeRefId(RefId(new NodeX)))
    // `getOrElse`, not the `Option` itself: the private runtime's `Some` has
    // no case-class `toString` and the two modes would print differently.
    println(fetched(new Comprehension(Some("hi"))).getOrElse("-"))
    println(fetched(new Comprehension(None)).getOrElse("-"))

    // (5) `Properties(int)` does not take `null`, and `Hashtable(Map)` is not
    // an alternative of `Properties`'s constructor at all.
    val p = new java.util.Properties(null)
    p.setProperty("k", "v")
    println(p.getProperty("k"))

    // The argument's class reached only here: `java.io.PrintStream` has to be
    // read off the classpath before it conforms to `OutputStream`.
    val w = new PrintWriter(new OutputStreamWriter(System.out))
    w.println("wrote")
    w.flush()
  }
}
