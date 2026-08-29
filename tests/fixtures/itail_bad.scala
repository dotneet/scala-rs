// implicit が見つからない場合。`itail.scala` で通るようになった経路が
// 「見つからなくても通る」ようになっていないことを固定する。
//
// 1. 引数位置の残余 implicit 節は、パラメータ型が要求する証拠で埋める。
//    スコープにある唯一の implicit を代わりに使ってはいけない。
// 2. 値引数が触れない型パラメータは implicit 探索が決めるが、候補が
//    まったくないなら決まらない。

class Tagged[T](val name: String)
object Tagged {
  implicit val intTag: Tagged[Int] = new Tagged[Int]("int")
}

class Sized[T](val n: Int)

object Bad {
  def take(xs: Sized[String]): Int = xs.n
  def empty[T](implicit t: Tagged[T]): Sized[T] = new Sized[T](0)

  // `Tagged[String]` は存在しないので、残余 implicit 節は埋まらない。
  val a: Int = take(empty)

  def rows[T](prefix: String)(implicit sz: Sized[T]): String = prefix + sz.n

  // `Sized` の implicit はどこにもないので、`T` は決まらない。
  val b = rows("p")
}
