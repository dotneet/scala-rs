// Custom extractors: unapply returning Option, Boolean, a tuple, a
// name-based result (isEmpty/get), unapplySeq, and extractor side effects.
object Main {
  var calls = 0
  object Even { def unapply(n: Int): Boolean = { calls += 1; n % 2 == 0 } }
  object Half { def unapply(n: Int): Option[Int] = if (n % 2 == 0) Some(n / 2) else None }
  object Split { def unapply(s: String): Option[(String, String)] = s.indexOf('=') match { case -1 => None; case i => Some((s.take(i), s.drop(i + 1))) } }
  object Words { def unapplySeq(s: String): Option[Seq[String]] = Some(s.split(" ").toSeq.filter(_.nonEmpty)) }

  final class NameOpt(val s: String) { def isEmpty: Boolean = s.isEmpty; def get: String = s.reverse }
  object Rev { def unapply(s: String): NameOpt = new NameOpt(s) }

  final class PairRes(a: Int, b: Int) { def isEmpty = false; def get = this; def _1 = a; def _2 = b }
  object DivMod { def unapply(n: Int): PairRes = new PairRes(n / 3, n % 3) }

  case class Email(user: String, domain: String)
  object Email2 { def unapply(s: String): Option[Email] = s.split("@") match { case Array(u, d) => Some(Email(u, d)); case _ => None } }

  class Temp(val c: Double)
  object Temp { def unapply(t: Temp): Some[Double] = Some(t.c * 9 / 5 + 32) }

  def show(x: Any): String = x match {
    case Even() => "even"
    case _ => "odd-or-other"
  }

  def main(args: Array[String]): Unit = {
    println(List(1, 2, 3, 4).map(show) + " calls=" + calls)
    List(8, 7, 12).foreach {
      case Half(Half(q)) => println("quarter " + q)
      case Half(h) => println("half " + h)
      case n => println("odd " + n)
    }
    List("k=v", "novalue", "a=b=c").foreach {
      case Split(k, v) => println(s"key=$k value=$v")
      case s => println("no split: " + s)
    }
    List("hello big world", "one", "", "a b").foreach {
      case Words() => println("no words")
      case Words(w) => println("one word " + w)
      case Words(a, b) => println(s"two $a $b")
      case Words(a, rest @ _*) => println(s"many $a + ${rest.length}")
    }
    List("abc", "").foreach {
      case Rev(r) => println("rev " + r)
      case _ => println("empty")
    }
    "me@example.org" match { case Email2(Email(u, d)) => println(u + " at " + d); case _ => println("bad") }
    new Temp(100) match { case Temp(f) => println(f) }
  }
}
