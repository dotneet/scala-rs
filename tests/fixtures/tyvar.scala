// 型変数の遅延解決（nsc の undetermined type variables）。
//
// 引数はオーバーロード解決のために期待型なしで型付けするので、`Map.empty` の
// ような多相参照は自分の型パラメータを抱えたまま（`Map[K, V]`）引数位置に来る。
// nsc はそれを「未確定の型変数」として持ち回り、呼び出しの候補を選び終えてから
// パラメータ型で一度に解く。以下はすべて実 scalac 2.13.16 が通す形。

object Empties {
  def m(m: Map[String, Int]): Int = m.size
  def v(v: Vector[String]): Int = v.length
  def s(s: Set[Int]): Int = s.size
  def l(xs: List[Int], ys: List[Int]): Int = xs.size + ys.size
  def sq(xs: Seq[String]): Int = xs.length

  val a = m(Map.empty)
  val b = v(Vector.empty)
  val c = s(Set.empty)
  val d = l(List.empty, Nil)
  val e = sq(Seq.empty)
}

// 空の `apply`（`Map()` / `Vector()`）も同じ形。結果型が自分の型パラメータを
// 抱えたまま返る。
object EmptyApplies {
  def m(m: Map[String, Int]): Int = m.size
  def v(v: Vector[String]): Int = v.length
  def l(xs: List[String]): Int = xs.length

  val a = m(Map())
  val b = v(Vector())
  val c = l(List())
}

// 未確定の型変数は入れ子の呼び出しからも漏れてくる。`id` の `T` は
// `Map[K, V]` に解けるが、`K` と `V` は外側のパラメータ型が決める。
object Nested {
  def id[T](x: T): T = x
  def take(m: Map[String, Int]): Int = m.size
  val a = take(id(Map.empty))
}

// 結果型まで届いた変数は期待型が決める。`f(Map.empty)` の結果は
// `List[Map[?K, ?V]]` で、宣言した `List[Map[String, Int]]` が `?K` / `?V` を決める。
object FromExpected {
  def f[T](x: T): List[T] = List(x)
  val a: List[Map[String, Int]] = f(Map.empty)
}

// 可変長引数・by-name・デフォルト引数の位置も同じ経路。
object OtherPositions {
  def varargs(xs: Map[String, Int]*): Int = xs.length
  def byName(m: => Map[String, Int]): Int = m.size
  def withDefault(m: Map[String, Int] = Map.empty): Int = m.size

  val a = varargs(Map.empty, Map.empty)
  val b = byName(Map.empty)
  val c = withDefault()
  val d = withDefault(Map.empty)
}

// 引数が複数あっても、節が複数あっても同じ。
object Clauses {
  def two(n: Int, m: Map[String, Int]): Int = n + m.size
  def curried(m: Map[String, Int])(n: Int): Int = m.size + n
  val a = two(1, Map.empty)
  val b = curried(Map.empty)(2)
}

// オーバーロードの選択そのものが未確定の型変数越しに行われる。
object Overloaded {
  def f(x: Seq[Int]): Int = x.sum
  def f(x: String): Int = x.length
  val a = f(Seq.empty)
}

// コンストラクタ引数も同じ経路を通る。
class Box(val m: Map[String, Int], val v: Vector[String]) {
  def size: Int = m.size + v.length
}

object Ctor {
  val a = new Box(Map.empty, Vector.empty).size
}

// 呼び出し側の型パラメータのほうが未確定な場合（nsc の undetparams のもう半分）。
// `xs.collect { case … }` は `PartialFunction[Int, ?B]` に対して検査され、
// `?B` はリテラルの結果型が決める。ここを `Any` に潰してしまうと結果型が
// 壊れるので、引数から解いた解を使う。
object CalleeOpen {
  val xs = List(1, 2, 3, 4)
  val a: List[String] = xs.collect { case n if n % 2 == 0 => n.toString }
  val b: List[Int] = xs.map(n => n + 1)
  val c: Option[Int] = Some(3).collect { case n => n * 2 }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Empties.a)
    println(Empties.b)
    println(Empties.c)
    println(Empties.d)
    println(Empties.e)
    println(EmptyApplies.a)
    println(EmptyApplies.b)
    println(EmptyApplies.c)
    println(Nested.a)
    println(FromExpected.a)
    println(OtherPositions.a)
    println(OtherPositions.b)
    println(OtherPositions.c)
    println(OtherPositions.d)
    println(Clauses.a)
    println(Clauses.b)
    println(Overloaded.a)
    println(Ctor.a)
    println(CalleeOpen.a)
    println(CalleeOpen.b)
    println(CalleeOpen.c)
  }
}
