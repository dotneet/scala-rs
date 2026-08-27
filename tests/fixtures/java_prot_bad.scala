package other

class Unrelated {
  def bad(b: jprot.Base): Int = b.secret()
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Unrelated().bad(new jprot.Base()))
  }
}
