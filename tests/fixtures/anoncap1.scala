// Anonymous classes capturing enclosing-method parameters and locals.
trait Runner { def run(): Unit }
trait Adder { def add(k: Int): Int }
abstract class Tagged(val tag: String) { def label: String = "b:" + tag }

object Main {
  // A single captured parameter.
  def mk(x: Int): Runner = new Runner {
    def run(): Unit = println("mk " + x)
  }

  // Several captures at once: two parameters and a block-local val.
  def multi(a: Int, b: String): Adder = {
    val c = a * 2
    new Adder {
      def add(k: Int): Int = k + a + c + b.length
    }
  }

  // A capture used in the parent constructor call and in an override.
  def withBase(n: Int): Tagged = new Tagged("t" + n) {
    override def label: String = super.label + "/" + n
  }

  // A capture read from the anonymous class' own constructor body.
  def eager(n: Int): Adder = new Adder {
    val doubled: Int = n * 2
    def add(k: Int): Int = k + doubled
  }

  def main(args: Array[String]): Unit = {
    mk(7).run()
    println(multi(3, "abc").add(1))
    println(withBase(9).label)
    println(eager(6).add(1))
  }
}
