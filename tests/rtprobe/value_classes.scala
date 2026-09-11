// Value classes: methods, equality, toString, boxing into collections and
// generic code, pattern matching, arrays of value classes, and as type args.
object Main {
  class Meter(val v: Double) extends AnyVal {
    def +(o: Meter): Meter = new Meter(v + o.v)
    def scale(k: Int): Meter = new Meter(v * k)
    override def toString = s"${v}m"
  }
  case class Id(raw: Int) extends AnyVal { def next: Id = Id(raw + 1) }
  class Name(val s: String) extends AnyVal { def greet = "hello " + s }
  implicit class IntOps(private val i: Int) extends AnyVal { def double: Int = i * 2 }

  def ident[T](t: T): T = t
  def sumAll(ms: Seq[Meter]): Meter = ms.foldLeft(new Meter(0))(_ + _)

  trait Printable extends Any { def print: String = "P(" + toString + ")" }
  class Code(val c: Int) extends AnyVal with Printable { override def toString = "code" + c }

  def main(args: Array[String]): Unit = {
    val a = new Meter(1.5); val b = new Meter(2.0)
    println(a + b); println((a + b).scale(3)); println(a == new Meter(1.5)); println(a.hashCode == new Meter(1.5).hashCode)
    val id = Id(5)
    println(id); println(id.next); println(id == Id(5)); println(id.next.next.raw)
    println(new Name("vc").greet); println(21.double)
    println(ident(a)); println(ident(id).next)
    val lst = List(Id(3), Id(1), Id(2))
    println(lst.sortBy(_.raw)); println(lst.map(_.next)); println(lst.contains(Id(2)))
    println(sumAll(Seq(a, b, new Meter(0.5))))
    val arr = Array(Id(1), Id(2))
    arr(0) = Id(10)
    println(arr.toList)
    val any: Any = id
    any match { case Id(r) => println("matched " + r); case _ => println("no") }
    println(any.isInstanceOf[Id])
    val opt: Option[Meter] = Some(a)
    println(opt.map(_.scale(2)))
    val m = Map(Id(1) -> "one")
    println(m(Id(1)))
    println(new Code(7).print)
    val pr: Printable = new Code(8); println(pr.print)
    val f: Id => Id = _.next
    println(f(Id(0)))
  }
}
