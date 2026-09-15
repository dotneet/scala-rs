class Payload(val value: Int)
trait Holder { type Inner = Payload }
object Main {
  def main(args: Array[String]): Unit = {
    val result: String = new TaggedOwner[String](new TaggedOwner[Holder#Inner](new Payload(42)).choose("middle")).choose("done")
    println(result)
  }
}
