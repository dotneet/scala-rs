object Main {
 def mark(n:Int):Int={println(n);n}
 def main(args:Array[String]):Unit={
  val c=new ext.External(b=mark(2),a=mark(1))()
  println(c.a);println(c.b);println(c.c);println(c.d)
  val z=new ext.External()()
  println(z.a+z.b+z.c+z.d)
 }
}
