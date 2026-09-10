class C(val a:Int)(val b:Int=a+1);
object Main{def mark(n:Int):Int={println(n);
n};
def main(args:Array[String]):Unit={val c=new C(a=mark(3))();
println(c.a);
println(c.b)}}
