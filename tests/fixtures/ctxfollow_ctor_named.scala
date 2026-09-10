case class C(id:Int,path:String)(val repo:String,val issue:Int);
object C{def create:C=new C(id=1,path="x")("r",2)};
object Main{def main(args:Array[String]):Unit={val c=C.create;
println(c.id);
println(c.repo);
println(c.issue)}}
