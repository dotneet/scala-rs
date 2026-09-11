class Text(val s:String) extends AnyVal
class Direct(val value:Text){def read:String=value.s;def qualified:String=this.value.s}
class Generic[T](val value:T)
class Derived extends Generic[Text](new Text("generic")){def read:String=value.s;def qualified:String=this.value.s}
object Main{def main(args:Array[String]):Unit={val d=new Direct(new Text("direct"));println(d.read);println(d.qualified);val g=new Derived;println(g.read);println(g.qualified)}}
