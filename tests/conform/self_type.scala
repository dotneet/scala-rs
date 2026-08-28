trait Named { self =>
  def name: String
  def shout: String = self.name.toUpperCase
}
trait Sized { this: Named =>
  def size: Int
  def label: String = name + ":" + size
}
class Box extends Named with Sized {
  def name = "box"
  def size = 3
}
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box
    println(b.shout)
    println(b.label)
  }
}
