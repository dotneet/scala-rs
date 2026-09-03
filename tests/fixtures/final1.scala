// agent/final1: slick のコレクション引数まわり 6 根をまとめた fixture。
// 実 scalac 2.13.16 と同じ出力になることを dual-run で確認する。
import scala.collection.immutable
import scala.reflect.ClassTag

object Main {

  // (1) 自己別名 `self =>` は `C.this.type`。`self(i)` の `apply` は
  //     クラス自身のもの（slick `util/ConstArray.scala:276`）。
  //     (2) `toArray[R >: T : ClassTag]` の `R` は、期待型のないところでも
  //     下限に確定する（slick `jdbc/JdbcActionComponent.scala:725`）。
  final class CArr[+T](a: Array[Any], val length: Int) { self =>
    def apply(i: Int): T = a(i).asInstanceOf[T]
    def toSeq: immutable.IndexedSeq[T] = (0 until length).map(self(_))
    // 本体で `new Array[R](length)` を書かないのは、抽象な `R` の
    // `Array[R]` を `[java/lang/Object` という擬似クラス名で emit してしまう
    // codegen の既知の穴（本スライスの担当外）を踏まないため。
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

  // (3) 前方参照で遅延補完される def の本体は「引数を型付け中」ではない。
  //     `.flatten` の implicit 節が埋まらないまま推論結果になっていた
  //     （slick `jdbc/JdbcModelBuilder.scala:159`）。
  final case class Tbl(name: String, keys: Seq[String])
  final class TableBuilder(val raw: Seq[Seq[Int]]) {
    def buildModel(prefix: String) = Tbl(prefix, buildKeys(prefix))
    final def buildKeys(prefix: String) =
      raw.map(mf => if (mf.isEmpty) None else Some(prefix + mf.sum)).flatten
  }

  // (4) 2 つの引数が同じ型パラメータに寄与するとき、片方が持つ未確定変数は
  //     join の前に下限へ落とす（slick `compiler/MergeToComprehensions.scala:218`）。
  trait TermSym { def n: String }
  final case class Fld(n: String) extends TermSym

  // (5) case class でない普通のクラスは、コンパニオンに抽出子があるなら
  //     コンストラクタパターンではなく抽出子で照合する
  //     （slick `compiler/ExpandSums.scala:245`）。
  trait Node
  final case class Leaf(s: String) extends Node
  final case class Prod(ch: CArr[Node]) extends Node

  // (6) 反変でも共変でもない `Set` の要素は、期待型が引数より強い。
  trait ColOpt[+T] { def tag: String }
  final case class SqlType(s: String) extends ColOpt[Nothing] { def tag = "sql:" + s }
  case object AutoInc extends ColOpt[Nothing] { def tag = "auto" }
  final case class Column(name: String, options: Set[ColOpt[_]])

  def sqlOptions(dbType: Option[String]): Set[ColOpt[_]] =
    Set() ++ dbType.map(s => SqlType(s))

  def options(dbType: Option[String], autoInc: Boolean): Set[ColOpt[_]] =
    sqlOptions(dbType) ++ (if (autoInc) Some(AutoInc) else None)

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
    println(options(Some("VARCHAR"), autoInc = true).toList.map(_.tag).sorted.mkString(" "))
    println(Column("c", options(None, autoInc = false)).options.size)
  }
}
