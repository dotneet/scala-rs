// A value whose erasure is an interface (a trait) entering a slot whose
// erasure is one of the trait's *class* parents. Scala says it conforms; the
// JVM verifier cannot see an interface's class parent, so nsc's erasure casts
// (`adaptToType`). Every position a value can enter such a slot: a call
// receiver, an extractor, an argument, a local, a result, a field, an array
// element, and the join of an `if` / `match`.
import scala.util.matching.{Regex, UnanchoredRegex}

trait Tr { def t = "tr" }
class Base { def b = "base"; override def toString = "Base" }
trait Mix extends Base with Tr
class Impl extends Base with Mix
class Holder { val fr: Regex = Main.Date.unanchored; var fb: Base = Main.mix }

object Main {
  val Date = """(\d{4})-(\d{2})""".r
  def mix: Mix = new Impl
  def takeRegex(r: Regex): String = r.regex
  def takeBase(b: Base): String = b.b
  def asRegex: Regex = Date.unanchored
  def asBase: Base = mix
  def pick(flag: Boolean): Regex = if (flag) Date.unanchored else Date
  def pickM(n: Int): Base = n match { case 0 => mix; case _ => new Base }

  def main(args: Array[String]): Unit = {
    "on 2024-03 ok" match {
      case Date.unanchored(y, _) => println("u " + y)
      case _                     => println("no")
    }
    println(Date.unanchored.findFirstIn("x 2024-03"))
    println(Date.unanchored.regex)
    println(takeRegex(Date.unanchored))
    val u: UnanchoredRegex = Date.unanchored
    println(u.regex)
    val r: Regex = u
    println(r.regex)
    println(mix.b + " " + mix.t)
    println(takeBase(mix))
    val bb: Base = mix
    println(bb)
    println(asRegex.regex + " " + asBase.b)
    println(pick(true).regex + " " + pick(false).regex)
    println(pickM(0).b + " " + pickM(1).b)
    val h = new Holder
    println(h.fr.regex + " " + h.fb.b)
    h.fb = mix
    println(h.fb)
    val arr = Array[Regex](Date.unanchored, Date)
    println(arr.length + " " + arr(0).regex)
    val bs = Array[Base](mix)
    println(bs(0).b)
    val r2: Regex = if (args.length > 5) Date else Date.unanchored
    println(r2.regex)
    val b2: Base = if (args.length > 5) new Base else mix
    println(b2.b)
  }
}
