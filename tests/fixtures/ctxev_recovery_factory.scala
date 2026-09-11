// Independent programs share one real-scalac invocation.
package ctxev.recovery_factory.p0 {
trait Ev[A]{def value:A};class Const[A,B](val value:A);object Const{def empty[A:Ev,B]:Const[A,B]=new Const(implicitly[Ev[A]].value)};object Main{def f[A:Ev,B]:Const[A,B]=Const.empty;def main(args:Array[String]):Unit={implicit val ev:Ev[Int]=new Ev[Int]{def value=7};println(f[Int,String].value)}}
}
package ctxev.recovery_factory.p1 {
trait Ev[A]{def value:A};class Const[A,B](val value:A);object Const{def empty[A:Ev,B]:Const[A,B]=new Const(implicitly[Ev[A]].value)};object Main{def f[A:Ev,B]:Const[A,B]=Const.empty[A,B];def main(args:Array[String]):Unit={implicit val ev:Ev[Int]=new Ev[Int]{def value=7};println(f[Int,String].value)}}
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.recovery_factory.p0.Main.main(args)
  ctxev.recovery_factory.p1.Main.main(args)
} }
