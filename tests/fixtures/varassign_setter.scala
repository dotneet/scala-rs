// A field-like selection uses a hand-written setter when one is available.
// The object import also checks that a wildcard-imported variable goes through
// its module setter instead of writing the backing field directly.
class SetterBox {
  val value: Int = 1
  var seen: Int = 0
  def value_=(next: Int): Unit = { seen = next }
}

object ImportedState {
  var value: Int = 1
}

import ImportedState._

object Main {
  def main(args: Array[String]): Unit = {
    val box = new SetterBox
    box.value = 2
    value = 5
    println(box.seen)
    println(value)
  }
}
