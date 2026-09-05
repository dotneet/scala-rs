// The infix extractors are not universal: `+:` needs a sequence, and `#::`
// only matches the two lazy sequence types. Real scalac 2.13.16 rejects both
// of these.
object Main {
  def notASeq(x: Int): Int = x match {
    case h +: _ => h
    case _      => 0
  }

  def notLazy(xs: List[Int]): Int = xs match {
    case h #:: _ => h
    case _       => 0
  }

  def main(args: Array[String]): Unit = {
    println(notASeq(1))
    println(notLazy(Nil))
  }
}
