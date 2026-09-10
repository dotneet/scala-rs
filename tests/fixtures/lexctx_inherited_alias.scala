trait Root {type T;val value:T}
trait Base[A] extends Root {type T=List[A]}
class C extends Base[String] {val value=List("ok")}
object Main {def main(args:Array[String]):Unit=println(new C().value.head)}
