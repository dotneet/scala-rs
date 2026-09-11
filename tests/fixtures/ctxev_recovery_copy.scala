// Independent programs share one real-scalac invocation.
package ctxev.recovery_copy.p0 {
abstract class Chain[A];class NonEmpty[A](val v:A)extends Chain[A];case class Append[A](left:NonEmpty[A],right:NonEmpty[A]){def this(left:Chain[A],right:Chain[A])=this(left.asInstanceOf[NonEmpty[A]],right.asInstanceOf[NonEmpty[A]]);def copy(left:Chain[A],right:Chain[A]):Append[A]=new Append(left,right)};object Main{def main(args:Array[String]):Unit={val a=new NonEmpty(7);val b=new NonEmpty(8);println(new Append(a,b).copy(a,b).left.v)}}
}
package ctxev.recovery_copy.p1 {
object Calls{var n=0};abstract class Base[A];class Narrow[A](val n:A)extends Base[A];class Box[A](val x:Narrow[A],val y:Narrow[A]){def this(x:Base[A],y:Base[A])={this(x.asInstanceOf[Narrow[A]],y.asInstanceOf[Narrow[A]]);Calls.n+=1}};object Main{def f[A](x:Base[A],y:Base[A]):Box[A]=new Box(x,y);def main(args:Array[String]):Unit={val a=new Narrow(7);new Box(a,a);println(Calls.n);val b:Base[Int]=a;new Box(b,b);println(Calls.n);println(f(b,b).x.n);println(Calls.n)}}
}
package ctxev.recovery_copy.p2 {
case class C(x:Int){def copy(s:String):C=C(s.length)};object Main{def main(a:Array[String]):Unit=println(C(7).copy("abcd").x)}
}
package ctxev.recovery_copy.p3 {
case class C(x:Int){def copy(s:String="override"):C=C(s.length)};object Main{def main(a:Array[String]):Unit=println(C(7).copy().x)}
}
package ctxev.recovery_copy.p4 {
case class C(x:Int){val copy:String=>C=s=>C(s.length)};object Main{def main(a:Array[String]):Unit=println(C(7).copy("abcd").x)}
}
package ctxev.recovery_copy.p5 {
case class C(x:Int){private def copy(s:String):C=C(s.length);def run:Int=copy("abc").x};object Main{def main(a:Array[String]):Unit=println(C(7).run)}
}
package ctxev.recovery_copy.p6 {
trait P{def copy(s:String):Int=s.length};case class C(x:Int)extends P;object Main{def main(a:Array[String]):Unit=println(C(7).copy("abcd"))}
}
package ctxev.recovery_copy.p7 {
trait P{def copy(x:Int):C};case class C(x:Int)extends P;object Main{def main(a:Array[String]):Unit=println(C(7).copy(4).x)}
}
package ctxev.recovery_copy.curried {
case class C(x: Int)(val y: Int) { def copy(s: String)(t: String): Int = s.length + t.length }
object Main { def main(args: Array[String]): Unit = println(C(1)(2).copy("abc")("defg")) }
}
package ctxev.recovery_copy.ordinary {
object O { def copy(n: Int)(s: String): Int = n + s.length }
object Main { def main(args: Array[String]): Unit = println(O.copy(3)("abcd")) }
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.recovery_copy.p0.Main.main(args)
  ctxev.recovery_copy.p1.Main.main(args)
  ctxev.recovery_copy.p2.Main.main(args)
  ctxev.recovery_copy.p3.Main.main(args)
  ctxev.recovery_copy.p4.Main.main(args)
  ctxev.recovery_copy.p5.Main.main(args)
  ctxev.recovery_copy.p6.Main.main(args)
  ctxev.recovery_copy.p7.Main.main(args)
  ctxev.recovery_copy.curried.Main.main(args)
  ctxev.recovery_copy.ordinary.Main.main(args)
} }
