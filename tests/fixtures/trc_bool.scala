// The right operand of `scala.Boolean.&&` and `scala.Boolean.||` is a tail
// position. nsc's TailCalls phase special-cases `Boolean_and` / `Boolean_or`
// and transforms their argument in the tail context, because both compile to
// a conditional branch over the operand: nothing runs after it.
//
// Seven `@tailrec` methods of the 2.13.16 library are written this way, and
// each shape below reproduces one of them:
//   LinearSeq.sameElements / List.equals   `(a eq b) || { … rec(…) … }`
//   StringParsers.forAllBetween            `i >= end || pred && rec(i + 1)`
//   ListSet.containsInternal               `!n.isEmpty && (x || rec(…))`
//   Promise.tryComplete0                   `(p ne this) && p.rec(…)`
//   ClassManifestDeprecatedApis.subtype    `left.nonEmpty && { … rec(…) }`
//   sys.process.Parser.skipToDelim         a `match` arm ending in `… && rec()`
//
// The deep cases run two million iterations under a 256k stack, so they
// terminate only if the call really became a backward branch.
import scala.annotation.tailrec

final class TrcPing(val name: String) {
  var other: TrcPing = null
  // `Promise.tryComplete0`: the tail call changes the receiver, and reaching
  // it at all depends on `&&`'s right operand being a tail position.
  @tailrec final def bounce(n: Int): Boolean =
    (n <= 0) || (other ne null) && other.bounce(n - 1)
}

object TrcBool {
  var probes = 0
  def probe(b: Boolean): Boolean = {
    probes += 1
    b
  }

  // `StringParsers.forAllBetween`: `i >= end || pred && rec(i + 1)`.
  @tailrec def allEven(i: Int, end: Int): Boolean =
    i >= end || (i % 2 == 0 || i % 2 == 1) && allEven(i + 1, end)

  // `ListSet.containsInternal`: `&&` whose right operand is a parenthesised
  // `||` whose own right operand is the recursive call.
  @tailrec def reaches(n: Int, e: Int): Boolean =
    (n > 0) && (n == e || reaches(n - 1, e))

  // `sys.process.Parser.skipToDelim`: a `match` arm whose value is `x && {
  // …; rec(…) }`.
  @tailrec def scan(i: Int): Boolean = i match {
    case 0 => true
    case _ =>
      (i > 0) && {
        val j = i - 1
        scan(j)
      }
  }

  // `LinearSeq.sameElements` / `List.equals`: the recursive call sits inside a
  // block that is the right operand of `||`, under an `if`.
  @tailrec def sameTail(a: List[Int], b: List[Int]): Boolean =
    (a eq b) || {
      if (a.nonEmpty && b.nonEmpty && a.head == b.head) sameTail(a.tail, b.tail)
      else a.isEmpty && b.isEmpty
    }

  // `&&` must not evaluate its right operand when the left is false, and the
  // loop must not run either.
  @tailrec def andShort(n: Int): Boolean =
    (n <= 0) || probe(n % 2 == 0) && andShort(n - 1)

  // `||` must not evaluate its right operand when the left is true.
  @tailrec def orShort(n: Int): Boolean = probe(n <= 0) || orShort(n - 1)

  var wideAcc = 0L
  // Two-slot `Long` arguments stored on the back edge taken from inside the
  // short circuit. `&&` binds tighter than `||`, so the recursive call is the
  // right operand of the `||`.
  @tailrec def wideOr(n: Long, acc: Long): Boolean =
    (n <= 0L) && { wideAcc = acc; true } || wideOr(n - 1L, acc + n)

  def main(args: Array[String]): Unit = {
    println(allEven(0, 2000000))
    println(reaches(2000000, -1))
    println(scan(2000000))

    val a = TrcPing0.a
    println(a.bounce(2000000))

    println(sameTail(List(1, 2, 3), List(1, 2, 3)))
    println(sameTail(List(1, 2, 3), List(1, 2, 4)))
    println(sameTail(Nil, Nil))

    probes = 0
    println(andShort(3))
    println(probes)
    probes = 0
    println(andShort(4))
    println(probes)
    probes = 0
    println(orShort(3))
    println(probes)

    println(wideOr(2000000L, 0L))
    println(wideAcc)
  }
}

object TrcPing0 {
  val a = new TrcPing("a")
  val b = new TrcPing("b")
  a.other = b
  b.other = a
}
