// Reduced scalac 2.13.16 acceptance and runtime comparisons.
package ctxev.parent_implicitly {
trait Evidence[A] { def value:A }; class Parent[A](val ev:Evidence[A]); class Child[A](implicit ev:Evidence[A]) extends Parent[A](implicitly); object Main{def main(args:Array[String]):Unit={implicit val ev:Evidence[Int]=new Evidence[Int]{def value=7};println(new Child[Int].ev.value)}}

}
package ctxev.parent_inferred {
trait Apply[F[_]]{def value[A](a:A):F[A]};trait Applicative[F[_]] extends Apply[F];trait Semigroup[A]{def combine(a:A,b:A):A};trait Monoid[A] extends Semigroup[A];class Parent[F[_],A](f:Apply[F],s:Semigroup[A]){def combine(a:A,b:A):F[A]=f.value(s.combine(a,b))};class Child[F[_],A](f:Applicative[F],s:Monoid[A]) extends Parent(f,s);object Main{def main(args:Array[String]):Unit={val f=new Applicative[Option]{def value[A](a:A):Option[A]=Some(a)};val s=new Monoid[Int]{def combine(a:Int,b:Int)=a+b};println(new Child(f,s).combine(2,3).get)}}

}
package ctxev.parent_implicit_explicit {
trait Ev[A]{def v:A};class P[A](val e:Ev[A]);class C[A](implicit e:Ev[A]) extends P[A](implicitly[Ev[A]]);object Main{def main(args:Array[String]):Unit={implicit val e:Ev[Int]=new Ev[Int]{def v=7};println(new C[Int].e.v)}}
}
package ctxev.parent_overload {
class P[A](val v:A){def this(a:A,n:Int)=this(a)};class C extends P("ok",1);object Main{def main(args:Array[String]):Unit=println(new C().v)}
}
package ctxev.parent_lambda {
class P(val f:Int=>Int);class C extends P(x=>x+1);object Main{def main(args:Array[String]):Unit=println(new C().f(6))}
}
package ctxev.function_class {
class Fn[A,B](f:A=>B) extends (A=>B){def apply(a:A):B=f(a)};object Main{def consume[A,B](f:A=>B,a:A):B=f(a);def id[F[_,_],A,B](f:F[A,B]):F[A,B]=f;def main(args:Array[String]):Unit={val f:Function1[Int,String]=new Fn((i:Int)=>i.toString);println(consume(f,7));println(consume(id[Function1,Int,String](f),8))}}

}
package ctxev.function2_class {
object Main{def id[F[_,_,_],A,B,C](x:F[A,B,C]):F[A,B,C]=x;def use[A,B,C](f:(A,B)=>C,a:A,b:B):C=f(a,b);def main(args:Array[String]):Unit=println(use(id[Function2,Int,Int,Int]((a:Int,b:Int)=>a+b),2,3))}
}
package ctxev.member_same_type_name {
trait App[F[_]]{def pure[A](a:A):F[A]};abstract class Func[F[_],A]{self=>def F:App[F];def own:App[F]=self.F;def other[G[_]](g:Func[G,A]):App[G]=g.F};object Main{def main(args:Array[String]):Unit={val f=new Func[Option,Int]{def F:App[Option]=new App[Option]{def pure[A](a:A):Option[A]=Some(a)}};println(f.own.pure(7).get);println(f.other(f).pure(8).get)}}

}
package ctxev.self_inherited {
trait App[F[_]]{def pure[A](a:A):F[A]};abstract class Base[F[_],A];abstract class Func[F[_],A] extends Base[F,A]{self=>def F:App[F];def product[G[_]](g:Func[G,A]):(App[F],App[G])=(self.F,g.F)};object Main{def main(args:Array[String]):Unit={val f=new Func[Option,Int]{def F:App[Option]=new App[Option]{def pure[A](a:A):Option[A]=Some(a)}};println(f.product(f)._1.pure(7).get)}}
}
package ctxev.tuple_class {
object Main{def id[F[_,_],A,B](x:F[A,B]):F[A,B]=x;def use[A,B](x:(A,B)):(B,A)=x.swap;def main(args:Array[String]):Unit={val x:(Int,String)=(1,"ok");println(use(id[Tuple2,Int,String](x)));println(id[Tuple2,Int,String](x).swap)}}
}
package ctxev.parent_byname {
  class Parent(x: => String) { def value: String = x }
  class Child(y: String) extends Parent(y) { def direct: String = y }
  class LazyChild(y: => String) extends Parent(y)
  object Main { def main(args: Array[String]): Unit = {
    val strict = new Child("hi")
    println(strict.direct)
    println(strict.value)
    var calls = 0
    val lazyChild = new LazyChild({ calls += 1; "lazy" })
    println(calls)
    println(lazyChild.value)
    println(lazyChild.value)
    println(calls)
  } }
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.parent_implicitly.Main.main(args)
  ctxev.parent_inferred.Main.main(args)
  ctxev.parent_implicit_explicit.Main.main(args)
  ctxev.parent_overload.Main.main(args)
  ctxev.parent_lambda.Main.main(args)
  ctxev.function_class.Main.main(args)
  ctxev.function2_class.Main.main(args)
  ctxev.member_same_type_name.Main.main(args)
  ctxev.self_inherited.Main.main(args)
  ctxev.tuple_class.Main.main(args)
  ctxev.parent_byname.Main.main(args)
} }
