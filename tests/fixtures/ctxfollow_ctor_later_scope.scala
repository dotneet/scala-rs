class C(val a:Int)(val b:Int=a+1);
object Main{def main(args:Array[String]):Unit={val a=100;
val c=new C(a=3)();
println(c.b);
println(a)}}
