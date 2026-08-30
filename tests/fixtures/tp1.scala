// A trait-private method and its private mutable state, called only from
// other members of the same trait -- the exact shape of
// `slick.util.ReadAheadIterator` (slick_subset.sh found this: the interface
// declared `update` `ACC_PRIVATE | ACC_ABSTRACT`, illegal per JVMS 4.6).
trait ReadAheadIterator[T] {
  private[this] var state = 0 // 0: no data, 1: cached, 2: finished
  private[this] var cached: T = null.asInstanceOf[T]

  protected def fetchNext(): T
  protected def finish(): Unit = { state = 2 }

  private[this] def update(): Unit = {
    if (state == 0) {
      cached = fetchNext()
      if (state == 0) state = 1
    }
  }

  def hasNext: Boolean = {
    update()
    state == 1
  }

  def next(): T = {
    update()
    if (state == 1) {
      state = 0
      cached
    } else throw new java.util.NoSuchElementException("next on empty iterator")
  }
}

class Counter(max: Int) extends ReadAheadIterator[Int] {
  private[this] var n = 0
  protected def fetchNext(): Int = {
    if (n >= max) { finish(); 0 }
    else { n += 1; n }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val it = new Counter(3)
    while (it.hasNext) println(it.next())
  }
}
