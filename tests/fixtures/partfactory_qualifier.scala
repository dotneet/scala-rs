object Main {
  trait St[T] { def map[U](f: T => U): St[U]; def value: T }
  trait Sq[+T] { def toSt[B >: T]: St[B] }
  class Store[T](val value: T) extends St[T] {
    def map[U](f: T => U): St[U] = new Store[U](f(value))
  }
  class Sequence[+T](value: T) extends Sq[T] {
    def toSt[B >: T]: St[B] = new Store[B](value)
  }
  def bound(ts: Sq[String]): St[String] = {
    val st = ts.toSt
    st.map(x => x)
  }
  def main(args: Array[String]): Unit = println(bound(new Sequence[String]("ok")).value)
}
