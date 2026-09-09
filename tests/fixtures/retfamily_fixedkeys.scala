import scala.collection.immutable.{IntMap, LongMap}
object Main {
  def main(args: Array[String]): Unit = {
    val im = IntMap(1 -> 2)
    val same = im.map { case (k, v) => (k, v.toString) }
    println(same.isInstanceOf[IntMap[_]])
    println(same(1))
    val changed = im.map { case (k, v) => (k.toString, v + 1) }
    println(changed("1"))
    println(im.map { case (_, v) => v + 2 }.head)
    val lm = LongMap(1L -> 2)
    val lsame = lm.map { case (k, v) => (k, v.toString) }
    println(lsame.isInstanceOf[LongMap[_]])
    println(lsame(1L))
    val lchanged = lm.map { case (k, v) => (k.toString, v + 1) }
    println(lchanged("1"))
    println(lm.map { case (_, v) => v + 2 }.head)
  }
}
