// `@specialized` / `@unspecialized` are accepted and recorded on the symbol,
// and they change no answer: specialization is a performance annotation, so a
// program compiled with it computes what the same program computes without it.
// That is what makes accepting it sound while the phase itself is missing --
// no `Box$mcI$sp` is emitted here, and tests/spec_classfiles.sh is the ledger
// that keeps saying so. See docs/specialization.md.
class Box[@specialized(Int, Long) T](val value: T) {
  @scala.annotation.unspecialized def get: T = value
}

// No argument list: nsc reads that as every primitive value class.
class Cell[@specialized T](val value: T) {
  def show: String = value.toString
}

// A group, and a method type parameter, which carries the annotation too.
class Pair[@specialized(Specializable.Bits32AndUp) A](val a: A) {
  def with2[@specialized(Double) B](b: B): String = a.toString + ":" + b.toString
}

object Main {
  def id[@specialized(Int, AnyRef) A](a: A): A = a

  def main(args: Array[String]): Unit = {
    println(new Box(41).get + 1)
    println(new Cell("cell").show)
    println(new Pair(7).with2(2.5))
    println(id(3))
    println(id("three"))
  }
}
