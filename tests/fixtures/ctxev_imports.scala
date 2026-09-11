// Reduced scalac 2.13.16 acceptance and runtime comparisons.
package ctxev.import_local_signature {
object WebHook{sealed trait Event{def name:String};case object Push extends Event{def name="push"}};object Main{def main(args:Array[String]):Unit={{import WebHook._;def check(event:Event):String=event.name;println(check(Push))}}}

}
package ctxev.import_named_signature {
object O{trait Event{def name:String};object E extends Event{def name="ok"}};object Main{def main(args:Array[String]):Unit={import O.Event;def f(e:Event):String=e.name;println(f(O.E))}}
}
package ctxev.import_alias_sig {
object O{trait E};object Main{def main(args:Array[String]):Unit={import O._;type X=E;def f(x:X):Int=7;println(f(new E{}))}}
}
package ctxev.import_val {
class Obj {val n=7};object Main{def main(args:Array[String]):Unit={val o=new Obj;import o._;println(n)}}
}
object Main { def main(args: Array[String]): Unit = {
  ctxev.import_local_signature.Main.main(args)
  ctxev.import_named_signature.Main.main(args)
  ctxev.import_alias_sig.Main.main(args)
  ctxev.import_val.Main.main(args)
} }
