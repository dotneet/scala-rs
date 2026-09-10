class C(val a:Int,val b:Int={println(2);
2})(val c:Int,val d:Int={println(4);
4});
object Main{def mark(n:Int):Int={println(n);
n};
def main(args:Array[String]):Unit={val c=new C(a=mark(1))(c=mark(3));
println(c.a+c.b+c.c+c.d)}}
