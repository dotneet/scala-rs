trait Element { def id: Int }
trait Leaf extends Element
trait Structure extends Element
final case class A() extends Leaf with Structure { def id = 1 }
final case class B() extends Structure { def id = 2 }
final case class C() extends Leaf { def id = 3 }
final class Box[T](val value:T) {
 def map[U](f:T=>U):Box[U] = new Box(f(value))
 def flatMap[U](f:T=>Box[U]):Box[U] = f(value)
}
object Box { def failed[T](message:String):Box[T] = throw new Exception(message) }
object Main {
 def select(n:Int): Box[Element] = for {
  v <- new Box(n)
  result <- v match {
   case 1 => n match {case 1 => new Box(new A); case _ => new Box(new B)}
   case 2 => if (n > 0) new Box(new C) else Box.failed("bad")
   case 3 => new Box(new C)
   case _ => Box.failed("bad")
  }
 } yield result
 def main(args:Array[String]):Unit = { println(select(1).value.id); println(select(2).value.id); println(select(3).value.id) }
}
