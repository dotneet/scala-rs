object Main{implicit def cv(s:String):Int=s.length;
trait Base{def foo:Int};
trait Child extends Base{val foo:String="abc"}}
