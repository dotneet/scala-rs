package identitycustom {
 class Array[A](val value:A)
 class Function1[A,B](val value:B)
 class Tuple2[A,B](val value:B)
 class String(val value:scala.Int)
 class Int(val value:scala.Int)
}
object Main {
 def main(args:scala.Array[java.lang.String]):Unit={
  {
   import identitycustom._
   val a:Array[java.lang.String]=new Array[java.lang.String]("array")
   val f:Function1[java.lang.String,scala.Int]=new Function1[java.lang.String,scala.Int](7)
   val t:Tuple2[scala.Int,java.lang.String]=new Tuple2[scala.Int,java.lang.String]("tuple")
   val s:String=new String(8)
   val i:Int=new Int(9)
   val arrow:scala.Int => scala.Int=(x:scala.Int)=>x+1
   println(arrow(10))
   println(a.value);println(f.value);println(t.value);println(s.value);println(i.value)
  }
  val q:identitycustom.String=new identitycustom.String(10)
  val qa:identitycustom.Array[java.lang.String]=new identitycustom.Array[java.lang.String]("qualified")
  type Alias[A]=identitycustom.Array[A]
  val alias:Alias[java.lang.String]=new identitycustom.Array[java.lang.String]("alias")
  println(q.value);println(qa.value);println(alias.value)
  val a:scala.Array[scala.Int]=new scala.Array[scala.Int](2)
  a(0)=2; a(1)=3
  val fn:scala.Function1[scala.Int,scala.Int]=(x:scala.Int)=>x+1
  val pair:scala.Tuple2[scala.Int,java.lang.String]=(4,"tuple2")
  println(fn(a(0)));println(pair._2)
 }
}
