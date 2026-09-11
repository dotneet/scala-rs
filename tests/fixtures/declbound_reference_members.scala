class Text(val s:String) extends AnyVal
class Ref(val o:AnyRef) extends AnyVal
class Values(val ctor:Text){val field:Text=ctor;lazy val late:Text=ctor;def method:Text=ctor;def applied():Text=ctor;def local:String=field.s}
class Objects(val ctor:Ref){val field:Ref=ctor;lazy val late:Ref=ctor;def method:Ref=ctor;def local:AnyRef=field.o}
object Main{def main(args:Array[String]):Unit={val v=new Values(new Text("text"));println(v.ctor.s);println(v.field.s);println(v.late.s);println(v.method.s);println(v.applied().s);println(v.local);val o=new Objects(new Ref("ref"));println(o.ctor.o);println(o.field.o);println(o.late.o);println(o.method.o);println(o.local)}}
