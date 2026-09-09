import family.Derived
trait Client {
  val profile: Derived
  import profile.api._
  def run(): Unit = {
    println(4.returning)
  }
}

object Main extends Client { val profile: Derived = new Derived; def main(args: Array[String]): Unit = run() }
