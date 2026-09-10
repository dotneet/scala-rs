class C(val a:Int=1)(val b:Int=a+1)(val c:Int=b+1);
object Main{def main(args:Array[String]):Unit={val c=new C()()();
println(c.a);
println(c.b);
println(c.c)}}
