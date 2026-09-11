// scalac: illegal inheritance; self-type Main.C does not conform to
// Main.T's selftype Main.T with Main.Logging.
object Main {
  trait Logging { def log(s: String): Unit = () }
  trait T { self: Logging => def run(): Unit = log("x") }
  class C extends T
  def main(args: Array[String]): Unit = new C().run()
}
