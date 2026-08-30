// 3 件目の一般性の確認：Ordered とは無関係な、利用者が書いた implicit def が
// 関数型の implicit パラメータに eta 展開されて渡ること。多相な implicit def
// も、自分の implicit 引数を持つ implicit def も対象。私有ランタイムでも動く。
import scala.language.implicitConversions

class Tagged(val s: String) { override def toString: String = "<" + s + ">" }
trait Show[A] { def show(a: A): String }
class Wrap[B](val get: B)

object Main {
  implicit def intTagged(n: Int): Tagged = new Tagged("i" + n)
  implicit def boxAny[A](a: A)(implicit sh: Show[A]): Tagged = new Tagged(sh.show(a))
  implicit val showString: Show[String] = new Show[String] {
    def show(a: String): String = "s" + a
  }

  // 関数型の implicit パラメータ。implicit def しか候補が無い。
  def render[A](a: A)(implicit view: A => Tagged): String = view(a).toString
  // view bound も同じ経路に落ちる。
  def render2[A <% Tagged](a: A): String = a.toString
  // 入れ子：自分の implicit パラメータを内側の呼び出しへ渡し直す。
  def renderPair[A](a: A, b: A)(implicit view: A => Tagged): String =
    render(a) + "|" + render(b)

  // B は呼び出しのどこにも現れない（nsc の未決定型パラメータ）。値ではなく
  // *変換* が witness なので、その結果型から B を解くしかない。
  implicit def intWrap(n: Int): Wrap[String] = new Wrap("w" + n)
  def unwrap[A, B](a: A)(implicit view: A => Wrap[B]): B = view(a).get

  def main(args: Array[String]): Unit = {
    println(render(7))
    println(render("hi"))
    println(render2(7))
    println(render2("hi"))
    // 変換そのものも多相なまま効く。
    val t: Tagged = "zz"
    println(t)
    // 入れ子の implicit パラメータでも view が見つかる。
    println(renderPair(1, 2))
    println(renderPair("a", "b"))
    // 未決定型パラメータを view の結果型から解く。
    val u = unwrap(9)
    println(u.length.toString + " " + u)
  }
}
