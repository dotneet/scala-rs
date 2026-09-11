// asInstanceOf/isInstanceOf across class hierarchies, traits, boxed
// primitives (a boxed Int is not a Long), arrays, and generic erasure,
// including the ClassCastExceptions scalac's code throws.
object Main {
  trait Flier; class Bird extends Flier; class Penguin extends Bird; class Plane extends Flier
  def cce(f: => Any): String = try { f; "no exception" } catch { case _: ClassCastException => "CCE" }
  def castGen[T](a: Any): T = a.asInstanceOf[T]
  def main(args: Array[String]): Unit = {
    val b: Bird = new Penguin
    println(b.isInstanceOf[Penguin] + " " + b.isInstanceOf[Flier] + " " + b.isInstanceOf[Plane @unchecked] + " " + (new Plane).isInstanceOf[Bird @unchecked])
    println(cce(b.asInstanceOf[Penguin]) + " " + cce((new Bird: Any).asInstanceOf[Penguin]) + " " + cce((new Plane: Flier).asInstanceOf[Bird]))
    val one: Any = 1
    println(one.isInstanceOf[Int] + " " + one.isInstanceOf[Long] + " " + one.isInstanceOf[java.lang.Integer] + " " + one.isInstanceOf[Number])
    println(cce(one.asInstanceOf[Long]) + " " + cce(one.asInstanceOf[Int]) + " " + cce(one.asInstanceOf[String]) + " " + cce(one.asInstanceOf[Double]))
    println(1.asInstanceOf[Long] + " " + 1.5.asInstanceOf[Int] + " " + 'a'.asInstanceOf[Int] + " " + 300.asInstanceOf[Byte] + " " + (65: Int).asInstanceOf[Char])
    println(cce(castGen[String](1)) + " " + cce(castGen[Int]("s")) + " " + cce { val s: String = castGen[String](1); s })
    val x: Int = castGen[Int](5); println(x + 1)
    println(cce { val i: Int = castGen[Int](5L); i })
    val arr: Any = Array(1, 2)
    println(arr.isInstanceOf[Array[Int]] + " " + arr.isInstanceOf[Array[Long]] + " " + arr.isInstanceOf[Array[_]] + " " + cce(arr.asInstanceOf[Array[String]]))
    val sarr: Any = Array("a")
    println(sarr.isInstanceOf[Array[AnyRef]] + " " + sarr.isInstanceOf[Array[Object]] + " " + sarr.isInstanceOf[Array[CharSequence]])
    val lst: Any = List(1, 2)
    println(lst.isInstanceOf[List[_]] + " " + lst.isInstanceOf[Seq[_]] + " " + lst.isInstanceOf[Vector[_]] + " " + cce(lst.asInstanceOf[List[String]]))
    println(cce { val l = lst.asInstanceOf[List[String]]; l.head.length })
    println((null: Any).isInstanceOf[String] + " " + cce(null.asInstanceOf[String]) + " " + (null: Any).asInstanceOf[Bird])
    val fn: Any = (x: Int) => x + 1
    println(fn.isInstanceOf[Function1[_, _]] + " " + fn.isInstanceOf[Function2[_, _, _]] + " " + fn.asInstanceOf[Int => Int](4))
    val unit: Any = ()
    println(unit.isInstanceOf[Unit] + " " + unit.isInstanceOf[scala.runtime.BoxedUnit])
    println((3: Any) match { case n: Long => "long"; case n: Int => "int"; case _ => "?" })
    println(classOf[Penguin].isInstance(b) + " " + classOf[Bird].isAssignableFrom(classOf[Penguin]) + " " + classOf[Int].isInstance(1))
  }
}
