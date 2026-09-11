// Every class below is rejected by scalac 2.13.16, each on its own line.
// (A separate file from `libov_override_bad.scala`: these are typer errors,
// and scalac stops before its override checks once one is reported.)
import java.{util => ju}

// A mixin whose superclass the class's own superclass does not extend.
trait IterWrap[A] extends ju.AbstractCollection[A]
abstract class Y extends ju.HashMap[String, String] with IterWrap[String]
class T2 extends ju.ArrayList[String] with scala.util.control.NoStackTrace
// A member trait's self type read at the enclosing class: `K` is `String`
// in neither of these.
trait KSet[A]
trait MapLike[K] { protected trait GenKeys { this: KSet[K] => } }
trait SortedMapLike[K] extends MapLike[K] { protected class Bad extends KSet[String] with GenKeys }
abstract class IntMap extends MapLike[Int] { class Bad2 extends KSet[String] with GenKeys }
// A self alias is not a member of the template's type.
trait Fn[-T, +R] { self => }
class UsesAlias(val o: Fn[Int, Int]) { def bad = o.self }
