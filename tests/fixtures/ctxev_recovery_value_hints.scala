// Independent programs share one real-scalac invocation.
package ctxev.recovery_value_hints.p0 {
object Main{def id[A](a:A):A=a;def takes(s:String):Int={println(s);7};def main(args:Array[String]):Unit={val m=Map("x"->"one");implicit def intKey(i:Int):String="a";println(m.getOrElse("missing",3));println(m.getOrElse("missing",{val x=4;x}));println(m.getOrElse("missing",if(true)5 else 6));println(m.getOrElse("missing",try 7 catch{case _:Exception=>8}));println(m.getOrElse("missing",id(9)));println(m.getOrElse("missing",(10:String)));println(m.getOrElse("missing",takes(11)));println(m.getOrElse(1,"fallback"));println(m.updated("y",12)("y"))}}
}
package ctxev.recovery_value_hints.p1 {
class Box[A](val x:A);object Main{def main(args:Array[String]):Unit={val m=Map("x"->new Box("ok"));implicit def box(n:Int):Box[String]=new Box("converted");println(m.getOrElse("missing",3));println(m.updated("y",4)("y"))}}
}
package ctxev.recovery_value_hints.p2 {
object Main{def f(m:Map[String,Set[String]]):Set[String]={implicit def conv(n:Int):String="converted";m.getOrElse("missing",Set(3))};def main(args:Array[String]):Unit=println(f(Map.empty[String,Set[String]]).head)}
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.recovery_value_hints.p0.Main.main(args)
  ctxev.recovery_value_hints.p1.Main.main(args)
  ctxev.recovery_value_hints.p2.Main.main(args)
} }
