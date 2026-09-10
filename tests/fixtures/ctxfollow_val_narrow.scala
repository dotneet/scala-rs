object Main{trait Base{def item:AnyRef};
class C extends Base{val item="abc"};
trait F{def f:Int=>Int};
class D extends F{val f=x=>x+1};
def main(args:Array[String]):Unit={println(new C().item.length);
println(new D().f(2))}}
