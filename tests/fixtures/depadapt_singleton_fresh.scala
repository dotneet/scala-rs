class Box(val value:Int)
object Main {var calls=0;def make():Box={calls+=1;new Box(calls)};def id[A <:AnyRef](a:A):a.type=a
 def main(args:Array[String]):Unit={val x:Box=id(make());println(x.value);println(calls)} }

