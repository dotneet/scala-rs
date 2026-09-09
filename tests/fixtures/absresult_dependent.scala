trait Space { type T; val x: T }
trait Extractor { def extract(s: Space): s.T }
class Sub extends Extractor { def extract(s: Space) = s.x }
object Main {
  def main(args: Array[String]): Unit = {
    val s = new Space { type T = String; val x = "dependent" }
    println(new Sub().extract(s))
  }
}
