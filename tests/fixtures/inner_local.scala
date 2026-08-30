// Anonymous and local classes: `InnerClasses` must set `inner_name` to
// nothing (empty `getSimpleName`) for the anonymous class and to a real name
// for the local one, and both need `outer_class_info` left at zero — so
// `isMemberClass` is false for both, unlike an ordinary nested class.
//
// (`getDeclaringClass`/`getEnclosingClass` nullness would be worth asserting
// here too, but `AnyRef == null` crashes the private runtime on an unrelated,
// pre-existing bug — see the `nullEqualsCrashesPrivateRuntime` note in
// `innerclasses.rs`. `isMemberClass` alone already exercises the same
// `outer_class_info == 0` codepath.)
object Main {
  trait Shape { def area: Double }

  def make(): Shape = new Shape { def area = 1.0 }

  def main(args: Array[String]): Unit = {
    val anon = make()
    println(anon.getClass.getSimpleName == "")
    println(anon.getClass.isAnonymousClass)
    println(anon.getClass.isLocalClass)
    println(anon.getClass.isMemberClass)

    class LocalC(val n: Int)
    val lc = new LocalC(5)
    println(lc.getClass.isLocalClass)
    println(lc.getClass.isAnonymousClass)
    println(lc.getClass.isMemberClass)
  }
}
