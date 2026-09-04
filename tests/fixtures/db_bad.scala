// agent/dbio: named arguments to a parent constructor must not merely be
// accepted -- a wrong name still has to be diagnosed as before.
abstract class Act(_name: String, statement: String)

class Wrong extends Act(_name = "Wrong", stmt = "s")

object Main {
  def main(args: Array[String]): Unit = println(new Wrong)
}
