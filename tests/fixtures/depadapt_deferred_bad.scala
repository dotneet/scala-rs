object Main {case class Deferred[A](fa:()=>Function0[A]) extends Function0[A] {def apply():A=fa()()};val bad:()=>String=Deferred(()=>()=>1)}
