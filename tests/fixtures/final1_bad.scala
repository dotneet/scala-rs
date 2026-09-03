// agent/final1 の異常系。緩めた側が黙って通す方向に倒れていないことを押さえる。
// 実 scalac 2.13.16 もこの 2 件を拒否する。
object Main {
  // 自己別名を widen して `apply` を探すようにしたが、`apply` を持たない
  // クラスでは今まで通り「メンバではない」と言わなければならない。
  final class NoApply(val n: Int) { self =>
    def get(i: Int): Int = self(i)
  }

  // 期待型は不変位置で引数より強いが、それは引数の解が期待型に適合する
  // ときだけ。適合しないなら型不一致のまま。
  def wrong: Set[Int] = Set() ++ Some("x")

  // `Option.option2Iterable` は `Option[A] => Iterable[A]`。ワイルドカードが
  // 何にでも unify するせいで、解くものが何も無いのに「形が合った」ことに
  // なっていた。実 scalac もこれを拒否する。
  trait ColOpt[+T]
  final case class DefaultOpt[T](v: T) extends ColOpt[T]
  def notAView(d: Option[DefaultOpt[_]]): IterableOnce[ColOpt[Nothing]] = d

  def main(args: Array[String]): Unit = {
    println(new NoApply(1).get(0))
    println(wrong)
    println(notAView(None))
  }
}
