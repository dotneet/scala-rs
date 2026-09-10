object Main {
 def main(args:Array[String]):Unit={
   val ints=new java.util.ArrayList[Int]();ints.add(2);ints.add(3)
   var total=0;ints.forEach(n=>total+=n);println(total)
   val lower:java.util.function.Consumer[_ >:String]=(x:AnyRef)=>println(x)
   lower.accept("ok")
   val upper:java.util.function.Consumer[_ <:CharSequence]=(x:CharSequence)=>println(x)
   println(upper!=null)
   val c:java.util.function.Consumer[Int]=(n:Int)=>println(n+1);c.accept(7)
 }
}
