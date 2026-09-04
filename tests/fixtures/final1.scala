// agent/final1: a fixture collecting 6 roots around slick's collection arguments.
// A dual run confirms the output matches real scalac 2.13.16.
import scala.collection.immutable
import scala.reflect.ClassTag

object Main {

  // (1) The self alias `self =>` is `C.this.type`; the `apply` in `self(i)` is
  //     the class's own (slick `util/ConstArray.scala:276`).
  //     (2) The `R` of `toArray[R >: T : ClassTag]` settles on its lower bound
  //     even where there is no expected type (slick `jdbc/JdbcActionComponent.scala:725`).
  final class CArr[+T](a: Array[Any], val length: Int) { self =>
    def apply(i: Int): T = a(i).asInstanceOf[T]
    def toSeq: immutable.IndexedSeq[T] = (0 until length).map(self(_))
    // The body avoids writing `new Array[R](length)` so as not to hit a known
    // codegen hole (out of scope for this slice) that emits `Array[R]` for an
    // abstract `R` under the pseudo class name `[java/lang/Object`.
    def toArray[R >: T : ClassTag]: Array[R] = Array.tabulate[R](length)(i => self(i))
  }
  object CArr {
    def apply[T](v0: T, v1: T): CArr[T] = new CArr[T](Array[Any](v0, v1), 2)
    def unapplySeq[T](c: CArr[T]): Some[IndexedSeq[T]] = Some(c.toSeq)
  }

  final class Sess {
    def withPrepared[T](sql: String, names: Array[String] = new Array[String](0))(f: Int => T): T = f(1)
    def withPrepared[T](sql: String, idxs: Array[Int])(f: Int => T): T = f(2)
  }

  // (3) The body of a def completed lazily through a forward reference is not
  //     "typing an argument". `.flatten`'s implicit clause was left unfilled and
  //     became the inference result (slick `jdbc/JdbcModelBuilder.scala:159`).
  final case class Tbl(name: String, keys: Seq[String])
  final class TableBuilder(val raw: Seq[Seq[Int]]) {
    def buildModel(prefix: String) = Tbl(prefix, buildKeys(prefix))
    final def buildKeys(prefix: String) =
      raw.map(mf => if (mf.isEmpty) None else Some(prefix + mf.sum)).flatten
  }

  // (4) When two arguments contribute to the same type parameter, the
  //     undetermined variables one of them carries are lowered to their lower
  //     bound before the join (slick `compiler/MergeToComprehensions.scala:218`).
  trait TermSym { def n: String }
  final case class Fld(n: String) extends TermSym

  // (5) A plain, non-case class matches through the extractor rather than as a
  //     constructor pattern when its companion has one
  //     (slick `compiler/ExpandSums.scala:245`).
  trait Node
  final case class Leaf(s: String) extends Node
  final case class Prod(ch: CArr[Node]) extends Node

  // (6) The elements of `Set`, which is neither covariant nor contravariant:
  //     the expected type is stronger than the argument.
  trait ColOpt[+T] { def tag: String }
  final case class SqlType(s: String) extends ColOpt[Nothing] { def tag = "sql:" + s }
  case object AutoInc extends ColOpt[Nothing] { def tag = "auto" }
  final case class DefaultOpt[T](v: T) extends ColOpt[T] { def tag = "def:" + v }
  final case class Column(name: String, options: Set[ColOpt[_]])

  def sqlOptions(dbType: Option[String]): Set[ColOpt[_]] =
    Set() ++ dbType.map(s => SqlType(s))

  // (7) Continuing the above. `Set[ColOpt[Nothing]] ++ Option[DefaultOpt[_]]`
  //     used to go through because `Option.option2Iterable` unified "in shape
  //     only", with nothing left to solve; that made the monomorphic
  //     `++(IterableOnce[A]): Set[A]` applicable and collapsed the whole chain
  //     to `Set[ColOpt[Nothing]]`.
  def options(dbType: Option[String], autoInc: Boolean, dflt: Option[DefaultOpt[_]]): Set[ColOpt[_]] =
    Set() ++
      dbType.map(s => SqlType(s)) ++
      (if (autoInc) Some(AutoInc) else None) ++
      dflt

  def fuse(n: Node): String = n match {
    case Prod(CArr(a, b)) => "prod(" + describe(a) + "," + describe(b) + ")"
    case other            => describe(other)
  }
  def describe(n: Node): String = n match {
    case Leaf(s) => s
    case _       => "?"
  }

  def main(args: Array[String]): Unit = {
    val c = CArr[Node](Leaf("x"), Leaf("y"))
    // (1)
    println(c(0).asInstanceOf[Leaf].s + c(1).asInstanceOf[Leaf].s)
    // (2)
    val names = CArr("k1", "k2")
    println(new Sess().withPrepared("insert", names.toArray)(i => "sess" + i))
    // (3)
    println(new TableBuilder(Seq(Seq(1, 2), Nil, Seq(4))).buildModel("t"))
    // (4)
    val byName: Map[String, Vector[TermSym]] =
      Map("a" -> Vector(Fld("f1"), Fld("f2")))
    val fields = byName.getOrElse("a", Seq.empty)
    println(fields.map(f => (f.n, f.n.length)).mkString(","))
    println(byName.getOrElse("b", Seq.empty).map(_.n).mkString("[", ",", "]"))
    // (5)
    println(fuse(Prod(c)))
    // (6)
    println(
      options(Some("VARCHAR"), autoInc = true, Some(DefaultOpt(7)))
        .toList
        .map(_.tag)
        .sorted
        .mkString(" ")
    )
    println(Column("c", options(None, autoInc = false, None)).options.size)
  }
}
