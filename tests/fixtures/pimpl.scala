// Parent constructors whose argument list the source leaves out: implicit
// clauses and defaulted parameters have to be filled in the `extends` clause
// exactly as they are at a `new` call site, or codegen emits `Parent.<init>()`
// and the program dies with `NoSuchMethodError`.

trait TT[T] { def name: String }
object TT {
  implicit val ttInt: TT[Int] = new TT[Int] { def name = "Int" }
  implicit val ttStr: TT[String] = new TT[String] { def name = "String" }
}

trait Ord[T] { def tag: String }
object Ord {
  implicit val ordInt: Ord[Int] = new Ord[Int] { def tag = "ordInt" }
  implicit val ordStr: Ord[String] = new Ord[String] { def tag = "ordStr" }
}

// The slick shape: the parent's only clause is implicit, the child states a
// context bound and forwards its own evidence parameter.
class TypedRep[T](implicit val tpe: TT[T]) {
  def describe: String = "rep[" + tpe.name + "]"
}
class ConstColumn[T: TT] extends TypedRep[T]

// An explicit clause followed by an implicit one that takes two parameters.
class Tagged[T](val label: String)(implicit val tpe: TT[T], val ord: Ord[T]) {
  def show: String = label + ":" + tpe.name + "/" + ord.tag
}
class IntTagged extends Tagged[Int]("int")
class StrTagged[T: TT: Ord](l: String) extends Tagged[T](l)

// Defaulted parameters, all of them and only the trailing one.
class Sized(val n: Int = 7, val unit: String = "cm") {
  def size: String = n.toString + unit
}
class DefaultSized extends Sized
class HalfSized extends Sized(3)

// A default in the first clause and an implicit second clause.
class Boxed[T](val depth: Int = 2)(implicit val tpe: TT[T]) {
  def box: String = depth.toString + "x" + tpe.name
}
class StrBoxed[T: TT] extends Boxed[T]

object Main {
  def main(args: Array[String]): Unit = {
    println(new ConstColumn[Int].describe)
    println(new ConstColumn[String].describe)
    println(new TypedRep[Int].describe)
    println(new TypedRep[String]().describe)
    println(new IntTagged().show)
    println(new StrTagged[String]("s").show)
    println(new DefaultSized().size)
    println(new HalfSized().size)
    println(new StrBoxed[String].box)
    // An anonymous class is a template too: its parent clause is filled the
    // same way.
    val anon = new TypedRep[Int] {
      override def describe: String = "anon:" + tpe.name
    }
    println(anon.describe)
    println(new ConstColumn[Int].tpe.name)
  }
}
