// Independent programs share one real-scalac invocation.
package ctxev.recovery_inference.p0 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def f[A](e:Either[Int,Positioned[A]]):Close[A]=e.fold(n=>new Single[A](n.asInstanceOf[A]),identity);def main(args:Array[String]):Unit={println(f[Int](Left(7)).value);println(f[Int](Right(new Positioned(8))).value)}}
}
package ctxev.recovery_inference.p1 {
abstract class Eval[+A]{def value:A};case class Now[+A](value:A)extends Eval[A];class State[F[_],E,S,A](val run:F[(E,S)=>F[(S,A)]]);object State{def applyF[F[_],E,S,A](f:F[(E,S)=>F[(S,A)]]):State[F,E,S,A]=new State(f)};object Main{def f[E,S,A](g:(E,S)=>(S,A)):State[Eval,E,S,A]=State.applyF(Now((e,s)=>Now(g(e,s))));def main(args:Array[String]):Unit=println(f[Int,Int,Int]((e,s)=>(s,e+s)).run.value(3,4).value._2)}
}
package ctxev.recovery_inference.p2 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def f[A](e:Either[Int,Positioned[A]]):Close[A]=e.fold(n=>new Single[A](n.asInstanceOf[A]),x=>x);def main(args:Array[String]):Unit={println(f[Int](Left(7)).value);println(f[Int](Right(new Positioned(8))).value)}}
}
package ctxev.recovery_inference.p3 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def f[A](e:Either[Int,Positioned[A]]):Close[A]=e.fold[Close[A]](n=>new Single[A](n.asInstanceOf[A]),identity);def main(args:Array[String]):Unit={println(f[Int](Left(7)).value);println(f[Int](Right(new Positioned(8))).value)}}
}
package ctxev.recovery_inference.p4 {
abstract class Eval[+A]{def value:A};case class Now[+A](value:A)extends Eval[A];class State[F[_],E,S,A](val run:F[(E,S)=>F[(S,A)]]);object State{def applyF[F[_],E,S,A](f:F[(E,S)=>F[(S,A)]]):State[F,E,S,A]=new State(f)};object Main{def f[E,S,A](g:(E,S)=>(S,A)):State[Eval,E,S,A]=State.applyF[Eval,E,S,A](Now((e,s)=>Now(g(e,s))));def main(args:Array[String]):Unit=println(f[Int,Int,Int]((e,s)=>(s,e+s)).run.value(3,4).value._2)}
}
package ctxev.recovery_inference.p5 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def main(a:Array[String]):Unit={val e:Either[Int,Positioned[Int]]=Left(7);val out=e.fold(n=>new Single(n),identity);val close:Close[Int]=out;println(close.value)}}
}
package ctxev.recovery_inference.p6 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def f[A](e:Either[Positioned[A],Int]):Close[A]=e.fold(identity,n=>new Single(n.asInstanceOf[A]));def main(a:Array[String]):Unit=println(f[Int](Right(8)).value)}
}
package ctxev.recovery_inference.p7 {
trait Close[+A]{def value:A};class Single[+A](val value:A)extends Close[A];class Positioned[+A](val value:A)extends Close[A];object Main{def f[A](e:Either[Int,Positioned[A]]):Close[A]={val keep:Positioned[A]=>Positioned[A]=x=>x;e.fold(n=>new Single(n.asInstanceOf[A]),keep)};def main(a:Array[String]):Unit=println(f[Int](Left(9)).value)}
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.recovery_inference.p0.Main.main(args)
  ctxev.recovery_inference.p1.Main.main(args)
  ctxev.recovery_inference.p2.Main.main(args)
  ctxev.recovery_inference.p3.Main.main(args)
  ctxev.recovery_inference.p4.Main.main(args)
  ctxev.recovery_inference.p5.Main.main(args)
  ctxev.recovery_inference.p6.Main.main(args)
  ctxev.recovery_inference.p7.Main.main(args)
} }
