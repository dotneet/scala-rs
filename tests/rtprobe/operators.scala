// Operators: right-associative (ending in ':') evaluation order, unary
// prefix operators, assignment operators desugared to `x = x op y` or to a
// `op=` method, precedence by first character, and symbolic method names.
object Main {
  val log = new StringBuilder
  def e[T](t: T): T = { log.append(t).append(' '); t }
  case class V(x: Int, y: Int) {
    def +(o: V) = V(x + o.x, y + o.y); def *(k: Int) = V(x * k, y * k); def unary_- = V(-x, -y)
    def unary_! : Boolean = x == 0 && y == 0; def ::(k: Int) = V(x + k, y); def +:(s: String) = s + toString
  }
  class Acc { var total = 0; def +=(n: Int): Acc = { total += n; this } }
  class Reg { var v = 1; def ^^(o: Int) = v * 100 + o }
  def main(args: Array[String]): Unit = {
    println(e(1) :: e(2) :: e(Nil)); println(log); log.clear()
    val l = e(List(3)); val r = e(0) +: l :+ e(9); println(r + " " + log); log.clear()
    println(V(1, 2) + V(3, 4) * 2); println(-V(1, -1)); println(!V(0, 0) + " " + !V(1, 0)); println(5 :: V(1, 1)); println("pre-" +: V(0, 1))
    var x = 10; x += 5; x -= 3; x *= 2; x /= 4; x %= 4; x <<= 3; x >>= 1; x |= 1; x &= 13; x ^= 2
    println(x)
    var s = "a"; s += "b"; s += 'c'; s += 1; println(s)
    var d = 1.5; d *= 2; d -= 0.5; println(d)
    val acc = new Acc; acc += 3; acc += 4; println(acc.total)
    var vv = V(1, 1); vv += V(1, 1); vv *= 3; println(vv)
    var li = List(1); li ::= 0; li :+= 2; li ++= List(3); println(li)
    println(1 + 2 * 3 - 4 / 2 % 3 + " " + (1 < 2 == true) + " " + (3 & 5 | 2 ^ 1) + " " + (true || false && false) + " " + (-2 max 3 min 1))
    println(new Reg ^^ 5)
    println(!true + " " + ~5 + " " + -(-3) + " " + +4 + " " + -5.abs)
    val m = scala.collection.mutable.Map(1 -> 1); m(1) += 10; m(2) = m.getOrElse(2, 0) + 1; println(m.toList.sorted)
    val arr = Array(1, 2); arr(0) *= 7; arr(1) -= 5; println(arr.toList)
    var bld = new StringBuilder; bld ++= "x"; bld += 'y'; println(bld)
    var ch = 'a'; ch = (ch + 1).toChar; println(ch)
    var lng = 1L; lng += 1; lng *= Int.MaxValue; println(lng)
    var bt: Byte = 10; bt = (bt * 20).toByte; println(bt)
    var fl = 1.0f; fl += 1; fl /= 3; println(fl)
    var set = Set(1); set += 2; set -= 1; set ++= Set(5); println(set)
    println(("a" -> 1) + " " + (1 -> 2 -> 3))
  }
}
