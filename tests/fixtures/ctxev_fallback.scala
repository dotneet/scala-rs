// Reduced scalac 2.13.16 acceptance and runtime comparisons.
package ctxev.getorelse_invariant {
object Main{def value[A](m:Map[Int,Set[A]]):Set[A]=m.getOrElse(0,Set());def main(args:Array[String]):Unit={println(value(Map(0->Set("ok"))).head);println(value(Map.empty[Int,Set[Int]]).isEmpty)}}

}
package ctxev.getorelse_jdbc {
object Main {def add[A](acc:Map[A,Set[A]],e:(A,A)):Map[A,Set[A]]=acc+(e._1->acc.getOrElse(e._1,Set()))+(e._2->(acc.getOrElse(e._2,Set())+e._1));def main(args:Array[String]):Unit=println(add(Map.empty[Int,Set[Int]],(1,2))(2).head)}


}
package ctxev.fallback_local {
object Main{def f[A](m:Map[Int,Set[A]]):Set[A]={val x=m.getOrElse(0,Set());val y:Set[A]=x;y};def main(args:Array[String]):Unit=println(f(Map.empty[Int,Set[String]]).isEmpty)}
}
package ctxev.getorelse_wider {
object Main{def main(args:Array[String]):Unit={val m:Map[Int,Set[String]]=Map(1->Set("x"));val got:Set[_]=m.getOrElse(0,Set(7));println(got.head);val any:Any=m.getOrElse(0,9);println(any)}}

}
object Main { def main(args: Array[String]): Unit = {
  ctxev.getorelse_invariant.Main.main(args)
  ctxev.getorelse_jdbc.Main.main(args)
  ctxev.fallback_local.Main.main(args)
  ctxev.getorelse_wider.Main.main(args)
} }
