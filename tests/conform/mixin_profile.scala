// Trait linearization, `super` through a stack of mixins, an abstract override,
// a self type, and an inner class -- the shape slick's profile cake uses.
object Main {
  trait Emitter { def emit(s: String): String }
  trait Base extends Emitter { def emit(s: String) = s }
  trait Quoted extends Emitter { abstract override def emit(s: String) = "\"" + super.emit(s) + "\"" }
  trait Upper extends Emitter { abstract override def emit(s: String) = super.emit(s).toUpperCase }
  trait Trimmed extends Emitter { abstract override def emit(s: String) = super.emit(s).trim }

  class A extends Base with Quoted with Upper
  class B extends Base with Upper with Quoted
  class C extends Base with Trimmed with Quoted with Upper

  trait Profile {
    type Col
    def col(name: String): Col
    def render(c: Col): String
    class Table(val name: String) {
      def qualified(c: String): String = name + "." + render(col(c))
    }
    def table(n: String): Table = new Table(n)
  }
  object PgProfile extends Profile {
    type Col = String
    def col(name: String) = name
    def render(c: String) = "\"" + c + "\""
  }
  object MyProfile extends Profile {
    type Col = (String, Int)
    def col(name: String) = (name, name.length)
    def render(c: (String, Int)) = "`" + c._1 + "`/" + c._2
  }

  trait HasName { def name: String }
  trait Loud { self: HasName =>
    def shout: String = name.toUpperCase + "!"
  }
  class Person(val name: String) extends HasName with Loud

  abstract class Shape { def area: Double; override def toString = getClass.getSimpleName + "(" + area + ")" }
  class Sq(s: Double) extends Shape { val area = s * s }
  class Ci(r: Double) extends Shape { def area = math.Pi * r * r }

  def main(args: Array[String]): Unit = {
    println(new A().emit(" hi "))
    println(new B().emit(" hi "))
    println(new C().emit(" hi "))
    println(PgProfile.table("t").qualified("c"))
    println(MyProfile.table("t").qualified("col"))
    val ps: List[Profile] = List(PgProfile, MyProfile)
    println(ps.map(p => p.render(p.col("x"))))
    println(new Person("ann").shout)
    val shapes: List[Shape] = List(new Sq(2), new Ci(1))
    println(shapes.map(_.toString))
    println(shapes.map(_.area).sum > 7)
    println(shapes.sortBy(_.area).map(_.getClass.getSimpleName))
    val t = PgProfile.table("u")
    println(t.name + " " + t.qualified("id"))
    println(new A().isInstanceOf[Emitter])
    println(classOf[A].getSimpleName)
  }
}
