object Main {
 trait B { def n:Int=3 }
 class A { self:B=>; def value:Int=n }
 trait Has[T] { def label:String="ok" }
 class G[T](val x:T) extends Has[T] { self:Has[T]=> }
 def main(args:Array[String]):Unit={
  println((new A with B).value)
  println(new G(4).x)
  println(new G[String]("yes").x)
  println(new G[Int](9).label)
 }
}
