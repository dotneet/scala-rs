// agent/dbio: slick の `JdbcActionComponent.scala` / `DBIOAction.scala` の根。
//
//  1. 親コンストラクタ呼び出しの名前付き引数
//     (`extends SimpleJdbcProfileAction[R](_name = …, statements = …)`)。
//  2. `private[this]` メンバを、外側クラスを *別の型引数で* 継承する匿名クラス
//     の中から無修飾で参照する
//     (`SynchronousDatabaseAction.superZip` / `superAsTry`)。
//  3. `[B >: A]` の下限が呼び出し側のメソッド型パラメタを含む場合の解
//     (`Either.getOrElse(throw …)` の受け側 `PositionedResultIterator[T]`)。
//     ここでは非 jar モードにもある `Option` で同じ形を通す。
//  4. 型付きパターン `case a: T[_, …]` は走査対象が既に言っている型引数を
//     保つ (nsc の `inferTypedPattern`)。
//
// 標準ライブラリを使わない `--no-scala-library` でも通るよう、
// `Vector` / `List` ではなく自前のクラスと `Array` だけで書いてある。

class Box[R](val first: R)

// 1. 親コンストラクタの名前付き引数。並べ替え・順序どおり・既定値の 3 形。
abstract class Act(_name: String, statement: String, repeat: Int = 1) {
  def show: String = {
    var s = _name + "["
    var i = 0
    while (i < repeat) {
      s = s + statement
      i = i + 1
    }
    s + "]"
  }
}

class Reordered(n: Int)
    extends Act(
      statement = if (n > 0) "one" else "all",
      _name = "Reordered"
    )

class InOrder(n: Int)
    extends Act(
      _name = "InOrder",
      statement = "s" + n.toString,
      repeat = 2
    )

// 2. `private[this]` の親メンバ。匿名サブクラスは `Outer[Box[R]]` なので、
//    `base` を「このクラスを通して」読むと `Box[Box[R]]` になってしまう。
//    `base` が public だと scalac も同じ mismatch を出す(継承したほうが
//    外側を隠す)ので、これは `private[this]` に固有の形。
abstract class Outer[R](val r: R) {
  private[this] def base: Box[R] = new Box[R](r)

  def wrap: String = {
    // 親コンストラクタ引数はローカルに逃がしてある(匿名クラスの `<init>`
    // の中で外側の `this` を読む形は、このスライスとは無関係の既知の
    // codegen バグ ―― `uninitializedThis` への `getfield` ―― を踏む)。
    val seed = new Box[R](r)
    val o = new Outer[Box[R]](seed) {
      val nonFused: Box[R] = base
      override def toString = "wrapped:" + nonFused.first.toString
    }
    o.toString
  }
}

// 4. `case a: T[_, …]` は走査対象が既に言っている型引数を保つ
//    (nsc の `inferTypedPattern`)。`Sync[_, _, _]` を裸で束縛すると
//    `superZip` の `Zip[R2, E2]` に渡せない。
trait Eff
trait Zip[+R, -E <: Eff] {
  def tag: String
  def zip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] =
    new Zipped[R2, E with E2]("plain(" + tag + "," + a.tag + ")")
}
class Zipped[R, E <: Eff](val tag: String) extends Zip[R, E]
trait Sync[+R, C, -E <: Eff] extends Zip[R, E] {
  private[this] def superZip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] =
    super.zip[R2, E2](a)
  override def zip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] = a match {
    case s: Sync[_, _, _] => new Zipped[R2, E with E2]("fused(" + superZip(s).tag + ")")
    case other            => superZip(other)
  }
}
class SyncAct[R](val tag: String) extends Sync[R, String, Eff]

object Main {
  // 3. 下限 `B >: A` の `A` が呼び出し側の型パラメタを含む形。引数は
  //    `Nothing` なので、下限を使わないと `B` が `Nothing` に解ける。
  def firstOf[T](o: Option[Box[T]]): T =
    o.getOrElse(throw new RuntimeException("empty")).first

  def main(args: Array[String]): Unit = {
    println(new Reordered(1).show)
    println(new Reordered(0).show)
    println(new InOrder(7).show)
    println(new Outer[String]("x") {}.wrap)
    println(firstOf(Some(new Box(41))))
    val sync = new SyncAct[Int]("sync")
    println(sync.zip(new SyncAct[Int]("other")).tag)
    println(sync.zip(new Zipped[Int, Eff]("plainRhs")).tag)
  }
}
