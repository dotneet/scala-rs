// A class/object nested inside a plain *class* (not an object): these carry
// an `$outer` field, so nsc leaves `ACC_STATIC` off their `InnerClasses`
// entry — unlike a class nested in a module. A `private` nested class must
// also report `private`, not `public`, in that entry.
class Outer {
  class Inner { def hi: String = "inner" }
  private class PrivC { def hi: String = "priv" }
  object InnerObj { def hi: String = "obj" }

  def report(): String = {
    val i = new Inner
    val p = new PrivC
    i.getClass.isMemberClass + "," +
      p.getClass.isMemberClass + "," +
      InnerObj.getClass.isMemberClass + "," +
      i.getClass.getSimpleName + "," +
      p.getClass.getSimpleName
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer().report())
  }
}
