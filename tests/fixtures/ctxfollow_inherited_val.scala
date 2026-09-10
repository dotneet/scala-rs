object Main{implicit def cv(s:String):Int=s.length;
trait Base{def foo:Int};
trait Child extends Base{val foo="abc"};
trait Generic[A]{val item:A};
trait GChild extends Generic[Int]{val item="abcd"};
class Concrete{def x:Int=1};
class C extends Concrete{override val x="five!"};
def main(args:Array[String]):Unit={println((new Child{}).foo);
println((new GChild{}).item);
println(new C().x)}}
