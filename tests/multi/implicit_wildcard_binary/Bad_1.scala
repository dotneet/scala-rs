// The rejection half: `toPlain` is a plain `def`, not an `implicit def`, so
// it is not a conversion and `3.plainly` has to stay an error. The guard
// supplies the pickled signatures of the members the pickle marks implicit;
// it must not turn the rest of the class into an implicit scope.
import iglib.TheProfile.api._

class BadSub(label: String) extends Leafy(label)

object Bad {
  def main(args: Array[String]): Unit = {
    println(new BadSub("x").label)
    println(3.plainly)
  }
}
