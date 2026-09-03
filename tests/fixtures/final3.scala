// agent/final3: four roots behind slick's remaining single diagnostics.
//
//  1. `Function[A, B]` is `Predef`'s alias for `A => B`; the bare name
//     otherwise resolves to the `scala.Function` *module*.
//  2. the least upper bound of two function types is a function type.
//  3. `C[_]` carries the declared bound of `C`'s own type parameter.
import scala.util.{Success, Try}

class Nd
final case class Compr[+Fetch <: Option[Nd]](tag: String) extends Nd

class MappedProjection(val label: String) {
  // 1: `Function[Any, Any]`, exactly as slick's `MappedProjection` writes it.
  def genericFastPath(f: Function[Any, Any]): String = f(label).toString
}

object Main {
  // 1 (cascade): with the parameter type unresolved the pattern-matching
  // anonymous function had no expected type -- "missing parameter type for
  // expanded function".
  def fastPath(mp: MappedProjection): String = mp.genericFastPath {
    case s: String => s.reverse
    case other     => other
  }

  // 2: `String => Int` and `String => String` join to `String => Any`, not to
  // `AnyRef` -- otherwise `fn(x)` is "value apply is not a member of AnyRef".
  val convertors = Seq(
    (s: String) => s.length,
    (s: String) => s.trim
  )

  // 3: `Compr[_]` is `Compr[_$1] forSome { type _$1 <: Option[Nd] }`, so
  // `Some(c)` is an `Option[Compr[Option[Nd]]]`.
  def fix(n: Nd, parent: Option[Compr[Option[Nd]]]): String = (n, parent) match {
    case (c: Compr[_], None) => fix(new Nd, Some(c))
    case (_, Some(c))        => "parent=" + c.tag
    case _                   => "none"
  }

  def main(args: Array[String]): Unit = {
    println(fastPath(new MappedProjection("abc")))
    println(convertors.map(fn => fn(" hi ").toString).mkString(","))
    println(fix(Compr[Option[Nd]]("c1"), None))
    val g: Predef.Function[Int, String] = (i: Int) => "n" + i
    println(g(7))
    println(Try(convertors.head(" x ")) match {
      case Success(v) => "ok:" + v
      case _          => "no"
    })
  }
}
