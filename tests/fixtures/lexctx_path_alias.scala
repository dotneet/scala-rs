trait Box {type T;val value:T}
class C(val other:Box) extends Box {type T=other.T;val value=other.value}
object N extends Box {type T=Int;val value=7}
object Main {def main(args:Array[String]):Unit=println(new C(N).value)}
