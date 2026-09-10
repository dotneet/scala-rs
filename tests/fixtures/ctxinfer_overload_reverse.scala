trait Base;
trait Sub extends Base;
class C{def choose(x:Int)(implicit b:Base):String="new";
protected def choose(x:Int)(s:Sub):String="old";
def call(x:Int,s:Sub):String={implicit def b:Base=s;
choose(x)}};
object Main{def main(args:Array[String]):Unit=println(new C().call(1,new Sub{}))}

