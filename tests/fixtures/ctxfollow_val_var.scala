object Main{implicit def cv(s:String):Int=s.length;
trait B{def x:Int};
class C extends B{var x="abc"};
def main(args:Array[String]):Unit={val c=new C;
c.x=4;
println(c.x)}}
