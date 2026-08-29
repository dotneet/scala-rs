// A bare constructor parameter is `private[this]`: it is not a member of the
// subclass, so `Sub`'s body cannot name `Outer`'s. scalac 2.13.16 rejects this
// with "not found: value tag".
class Outer(tag: String) { def outerTag: String = tag }
class Sub extends Outer("outer") { def stolen: String = tag }

object Main {
  def main(args: Array[String]): Unit = println(new Sub().stolen)
}
