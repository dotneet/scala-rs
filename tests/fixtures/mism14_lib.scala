// The shape slick's `GlobalConfig` hits: `asScala`'s conversion takes a
// `java.util.Map[K, V]`, and the receiver only *extends* one
// (`ConfigObject extends java.util.Map[String, ConfigValue]`). Solving `K` and
// `V` means reading the receiver's base type at `java.util.Map`, not zipping
// the receiver's own type arguments -- of which a plain subclass has none.
//
// `scala.jdk.CollectionConverters` lives in the library pickle, so this one is
// library-ABI only.
import scala.jdk.CollectionConverters._

class Names extends java.util.ArrayList[String]
class Props extends java.util.HashMap[String, Integer]

object Main {
  def take(m: scala.collection.mutable.Map[String, Integer]): String =
    m.getOrElse("a", Integer.valueOf(-1)).intValue.toString

  def main(args: Array[String]): Unit = {
    val ns = new Names
    ns.add("x")
    ns.add("y")
    val buf: scala.collection.mutable.Buffer[String] = ns.asScala
    println(buf.mkString(","))
    val ps = new Props
    ps.put("a", Integer.valueOf(7))
    println(take(ps.asScala))
    val pairs: Iterable[(String, Integer)] = ps.asScala
    println(pairs.size)
  }
}
