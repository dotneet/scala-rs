class Inv[T](val t: T)
class Cov[+T](val t: T)
class Con[-T] { def take(t: T): String = t.toString }
object Main {
  def widen(c: Cov[String]): Cov[Any] = c
  def narrow(c: Con[Any]): Con[String] = c
  def same(i: Inv[String]): Inv[String] = i
  def main(args: Array[String]): Unit = {
    println(widen(new Cov("a")).t)
    println(narrow(new Con[Any]).take("b"))
    println(same(new Inv("c")).t)
    val opt: Option[Any] = Some("d")
    println(opt)
  }
}
