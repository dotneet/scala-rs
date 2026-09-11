class Wrapped(val value:AnyRef) extends AnyVal
trait Base[T] {def get:T}
class Child extends Base[Wrapped] {def get:Wrapped=new Wrapped("hi")}
object Main {def main(args:Array[String]):Unit={val b:Base[Wrapped]=new Child;println(b.get.value);println(new Child().get.value)}}
