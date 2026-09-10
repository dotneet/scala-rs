trait Base;
trait Sub extends Base;
class C{protected def choose(x:Int)(s:Sub):String="old";
def choose(x:Int)(implicit b:Base):String="new";
def call(x:Int,s:Sub):String={implicit def b:Base=s;
choose(x)}};
object Main{def main(args:Array[String]):Unit=println(new C().call(1,new Sub{}))}

