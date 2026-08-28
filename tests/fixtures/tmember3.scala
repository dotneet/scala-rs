// Wildcards inside type-parameter bounds (`R <: Rep[?]`), higher-kinded type
// parameters passed as type arguments (`Query[?, U, C]`), and `#` projections
// applied to type arguments (`Profile#Table[?]`).

trait Rep[T] { def get: T }
class ConstRep[T](val get: T) extends Rep[T]

trait Query[E, U, C[_]] { def one: U }

trait Profile {
  trait AbstractTable[T] { def label: String }
  type Table[T] <: AbstractTable[T]
}

class CompiledFunction[F, R <: Rep[?], RU](val f: F)

object Main {
  def firstOf[R <: Rep[?]](r: R): String = r.get.toString
  def take[U, C[_]](q: Query[?, U, C]): U = q.one
  def tableName(t: Profile#AbstractTable[?]): String = t.label

  object P extends Profile {
    class Tbl extends AbstractTable[Int] { def label = "tbl" }
    type Table[T] = AbstractTable[T]
  }

  def main(args: Array[String]): Unit = {
    println(firstOf(new ConstRep(5)))
    val q: Query[String, Int, List] = new Query[String, Int, List] { def one = 9 }
    println(take[Int, List](q))
    println(tableName(new P.Tbl))
    println(new CompiledFunction[Int, ConstRep[Int], String](3).f)
  }
}
