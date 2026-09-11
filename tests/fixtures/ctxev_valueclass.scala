// Reduced scalac 2.13.16 acceptance and runtime comparisons.
package ctxev.value_self_argument {
class Cell[T](val value:T) extends AnyVal{def forwarded:T=Sink.take(this)};object Sink{def take[T](c:Cell[T]):T=c.value};object Main{def main(args:Array[String]):Unit={println(new Cell(7).forwarded);println(new Cell("text").forwarded)}}

}
package ctxev.value_self_local {
class Cell[T](val value:T) extends AnyVal{def local:T={val c:Cell[T]=this;c.value}};object Main{def main(args:Array[String]):Unit={println(new Cell(7).local);println(new Cell("text").local)}}

}
package ctxev.value_self_if {
class Cell[T](val value:T) extends AnyVal{def forwarded(b:Boolean):T=Sink.take(if(b)this else this)};object Sink{def take[T](c:Cell[T]):T=c.value};object Main{def main(args:Array[String]):Unit={println(new Cell(7).forwarded(true));println(new Cell("text").forwarded(false))}}
}
package ctxev.value_self_constructor {
class Sink[T](val c:Cell[T]);class Cell[T](val value:T)extends AnyVal{def f:T=new Sink[T](this).c.value};object Main{def main(args:Array[String]):Unit={println(new Cell(7).f);println(new Cell("text").f)}}
}
package ctxev.value_self_box {
class Box[A](val value:A);class Cell[T](val value:T) extends AnyVal{def boxed:Box[Cell[T]]=new Box(this)};object Main{def main(args:Array[String]):Unit={println(new Cell(7).boxed.value.value);println(new Cell("text").boxed.value.value)}}

}
package ctxev.value_self_any {
class Cell[T](val value:T)extends AnyVal{def f:Any=this};object Main{def main(args:Array[String]):Unit={println(new Cell(7).f.asInstanceOf[Cell[Int]].value);println(new Cell("text").f.asInstanceOf[Cell[String]].value)}}
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.value_self_argument.Main.main(args)
  ctxev.value_self_local.Main.main(args)
  ctxev.value_self_if.Main.main(args)
  ctxev.value_self_constructor.Main.main(args)
  ctxev.value_self_box.Main.main(args)
  ctxev.value_self_any.Main.main(args)
} }
