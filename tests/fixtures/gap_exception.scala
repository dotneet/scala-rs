// `java.lang.RuntimeException`/`Exception`/`Throwable` had no registered
// constructors or common methods at all (only the 0-arg constructor
// accidentally "worked"); this mirrors slick's own
// `class SlickException(msg: String, parent: Throwable = null)
//   extends RuntimeException(msg, parent)`, which needs both the
// `(String, Throwable)` constructor and an *omitted* trailing constructor
// default argument (`new SlickException("boom")`) to work.
class MyException(msg: String, parent: Throwable = null) extends RuntimeException(msg, parent)

object Main {
  def main(args: Array[String]): Unit = {
    val e = new MyException("boom")
    println(e.getMessage)
    println(e.getCause == null)
    val e2 = new MyException("boom2", new RuntimeException("cause"))
    println(e2.getMessage)
    println(e2.getCause.getMessage)
    try {
      throw new RuntimeException("thrown")
    } catch {
      case ex: RuntimeException => println(ex.getMessage)
    }
  }
}
