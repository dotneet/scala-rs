case class C(id:Int,path:String="x")(val repo:String,val issue:Int=2);
object C{def create:C=new C(id=1)("r")};
object Main{def main(args:Array[String]):Unit={val c=C.create;
println(c.path);
println(c.repo);
println(c.issue)}}
