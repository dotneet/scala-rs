// 未確定の型変数を「解けたことにしない」ことの固定。実 scalac 2.13.16 も
// 以下をすべて拒否する（メッセージの文言は違う）。

object WrongClass {
  def take(m: Map[String, Int]): Int = m.size
  // scalac: polymorphic expression cannot be instantiated to expected type;
  //         found [A]List[A]  required Map[String,Int]
  val bad = take(List.empty)
}

object WrongClass2 {
  def take(v: Vector[String]): Int = v.length
  val bad = take(Map.empty)
}

// 囲みのメソッドの型パラメータは「未確定の変数」ではなく確定した型。
// パラメータ型に合わせて勝手に解いてはいけない。
object EnclosingTparamIsNotAVariable {
  def take(m: Map[String, Int]): Int = m.size
  def g[K](m: Map[K, Int]): Int = take(m)
}

// 引数の型変数は、渡す先のパラメータ型で解けるときだけ解ける。
// `Map[T, Int]` に `Vector[A]` は当たらない。
object NoShapeMatch {
  def f[T](x: T, m: Map[T, Int]): Int = m.size
  def g[T](x: T): Int = f(x, Vector.empty)
}

// nsc も「undetermined type」と言って断る形。タプル要素の関数リテラルは
// `Tuple2.apply` の型パラメータが先に決まらないと型付けできない。
object UndeterminedInTuple {
  def f[A, B](p: (A, B => Int)): Int = 1
  val bad = f(("x", n => n + 1))
}
