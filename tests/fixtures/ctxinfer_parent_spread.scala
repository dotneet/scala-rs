class V(val n:Int);
class M(name:String,vs:V*){def value:Int=vs.map(_.n).sum};
object Main extends M("m",Seq(new V(1),new V(2)): _*){def main(args:Array[String]):Unit=println(value)}

