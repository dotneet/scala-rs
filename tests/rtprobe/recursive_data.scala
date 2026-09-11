// Recursive data: trees of case classes with List children, folds over
// them, a hand-written immutable linked list with variance, and structural
// equality of deep values.
object Main {
  case class Tree[A](value: A, children: List[Tree[A]] = Nil) {
    def size: Int = 1 + children.map(_.size).sum
    def depth: Int = 1 + (if (children.isEmpty) 0 else children.map(_.depth).max)
    def map[B](f: A => B): Tree[B] = Tree(f(value), children.map(_.map(f)))
    def fold[B](f: (A, List[B]) => B): B = f(value, children.map(_.fold(f)))
  }
  sealed trait MyList[+A] { def ::[B >: A](b: B): MyList[B] = Cons(b, this) }
  case object MyNil extends MyList[Nothing]
  case class Cons[+A](h: A, t: MyList[A]) extends MyList[A]
  def len[A](l: MyList[A]): Int = l match { case MyNil => 0; case Cons(_, t) => 1 + len(t) }
  def toList[A](l: MyList[A]): List[A] = l match { case MyNil => Nil; case Cons(h, t) => h :: toList(t) }
  sealed trait Json
  case class JNum(v: Double) extends Json; case class JStr(s: String) extends Json; case class JArr(xs: List[Json]) extends Json; case class JObj(kv: List[(String, Json)]) extends Json
  def render(j: Json): String = j match {
    case JNum(v) => if (v == v.toLong) v.toLong.toString else v.toString
    case JStr(s) => "\"" + s + "\""
    case JArr(xs) => xs.map(render).mkString("[", ",", "]")
    case JObj(kv) => kv.map { case (k, v) => "\"" + k + "\":" + render(v) }.mkString("{", ",", "}")
  }
  def main(args: Array[String]): Unit = {
    val t = Tree(1, List(Tree(2, List(Tree(4))), Tree(3)))
    println(t.size + " " + t.depth + " " + t.map(_ * 10) + " " + t.fold[Int]((v, cs) => v + cs.sum))
    println(t.fold[String]((v, cs) => if (cs.isEmpty) v.toString else s"$v(${cs.mkString(",")})"))
    println(t == Tree(1, List(Tree(2, List(Tree(4))), Tree(3))) + " " + (t.map(identity) == t) + " " + (t.hashCode == t.map(identity).hashCode))
    val ml = 1 :: 2 :: 3 :: MyNil
    println(ml + " " + len(ml) + " " + toList(ml))
    val widened: MyList[Any] = "s" :: ml
    println(toList(widened))
    println(render(JObj(List("a" -> JNum(1), "b" -> JArr(List(JStr("x"), JNum(2.5))), "c" -> JObj(Nil)))))
    def build(n: Int): Tree[Int] = if (n == 0) Tree(0) else Tree(n, List(build(n - 1), build(n - 1)))
    println(build(10).size)
  }
}
