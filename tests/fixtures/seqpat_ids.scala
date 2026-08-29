// Stable identifier patterns. nsc only demands that the pattern's type and the
// scrutinee could have a common instance, not that they conform: two open
// classes always could, so `case Ids.other =>` against an `ST[Int]` compiles.
//
// Also pins the modifiers of a definition that follows another class: the
// optional constructor modifier used to swallow them.
class Other
final class FinalOther
sealed class Sealed
abstract class Abstract { def n: Int }
trait Tr

class ST[T]

object Ids {
  val other = new Other
  val st: ST[Int] = new ST[Int]
  val tr: Tr = new Tr {}
}

object Main {
  def unrelated(x: ST[Int]): String = x match {
    case Ids.st => "st"
    case Ids.other => "other"
    case _ => "?"
  }

  def traitScrutinee(x: Tr): String = x match {
    case Ids.tr => "tr"
    case Ids.other => "other"
    case _ => "?"
  }

  def anyScrutinee(x: Any): String = x match {
    case Ids.other => "other"
    case _ => "?"
  }

  def main(args: Array[String]): Unit = {
    println(unrelated(Ids.st))
    println(unrelated(new ST[Int]))
    println(traitScrutinee(Ids.tr))
    println(traitScrutinee(new Tr {}))
    println(anyScrutinee(Ids.other))
    println(anyScrutinee(1))
    val a = new Abstract { def n = 7 }
    println(a.n)
    val f = new FinalOther
    val se = new Sealed
    println(f.toString.startsWith("FinalOther"))
    println(se.toString.startsWith("Sealed"))
  }
}
