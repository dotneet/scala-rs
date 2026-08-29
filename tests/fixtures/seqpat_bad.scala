// The relaxations must not swallow what scalac 2.13.16 still rejects.
class Other
final class FinalOther
trait Tr

class ST[T]

object Ids {
  val other = new Other
  val fin = new FinalOther
  val s = "x"
  val n = 3
}

object Main {
  // A final class cannot also be an `ST[Int]`.
  def finalVsClass(x: ST[Int]): Int = x match {
    case Ids.fin => 1
    case _ => 0
  }

  // `String` is final.
  def stringVsClass(x: Other): Int = x match {
    case Ids.s => 1
    case _ => 0
  }

  // A final class cannot mix in a trait it does not extend.
  def finalVsTrait(x: Tr): Int = x match {
    case Ids.fin => 1
    case _ => 0
  }

  // A primitive scrutinee admits no reference pattern.
  def intVsClass(x: Int): Int = x match {
    case Ids.other => 1
    case _ => 0
  }

  // ... and the other way round.
  def classVsInt(x: ST[Int]): Int = x match {
    case Ids.n => 1
    case _ => 0
  }

  def main(args: Array[String]): Unit = {
    println(finalVsClass(new ST[Int]))
    println(stringVsClass(new Other) + finalVsTrait(new Tr {}))
    println(intVsClass(1) + classVsInt(new ST[Int]))
  }
}
