// A capture must resolve to something in scope: the anonymous class body may
// not invent a name the enclosing method never defined.
trait Runner { def run(): Unit }

object Main {
  def mk(x: Int): Runner = new Runner {
    def run(): Unit = println(missingLocal + x)
  }

  def main(args: Array[String]): Unit = {
    mk(1).run()
  }
}
