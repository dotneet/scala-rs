class C(val a:Int,val b:Int)(val c:Int,val d:Int);
object Main{def mark(n:Int):Int={println(n);
n};
def main(args:Array[String]):Unit={val c=new C(b=mark(2),a=mark(1))(d=mark(4),c=mark(3));
println(c.a+c.b+c.c+c.d)}}
