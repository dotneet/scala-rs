// Left-to-right evaluation where the call shape runs right to left: the left
// operand of a right-associative operator runs before the receiver (except
// into a by-name parameter), and a constructor stores its parameter fields
// before the superclass constructor runs.
object Main {
  val log = new StringBuilder
  def e[T](t: T): T = { log.append(t).append(' '); t }
  def flush(): String = { val s = log.toString.trim; log.clear(); s }
  case class Box(xs: List[Int]) {
    def ::(x: Int)(implicit tag: String): Box = Box((x * 10) :: xs)
    def +:(x: => Int): Box = Box(x :: x :: xs)
  }
  implicit val tag: String = "t"
  abstract class Sup { val seen = describe; def describe: String }
  class Sub(val name: String, n: Int) extends Sup { val tag = "tag:" + name; def describe = s"name=$name n=$n tag=$tag" }
  def main(args: Array[String]): Unit = {
    println(e(1) :: e(2) :: e(Nil))
    println(flush())
    println(e("a") +: e(List("b")) :+ e("c"))
    println(flush())
    println(e(1) :: e(Box(Nil)))
    println(flush())
    var k = 0
    def bump(): Int = { k += 1; k }
    println(bump() +: e(Box(Nil)))
    println(flush() + " k=" + k)
    lazy val ll: LazyList[Int] = e(7) #:: e(8) #:: LazyList.empty
    println("before: [" + flush() + "]")
    println(ll.take(1).toList + " after: " + flush())
    val it = Iterator(1, 2, 3)
    println(it.next() :: it.next() :: it.next() :: Nil)
    println(List(1, 2, 3).foldLeft(List.empty[Int])((acc, x) => e(x) :: acc) + " " + flush())
    println(new Sub("n", 4).seen)
  }
}
