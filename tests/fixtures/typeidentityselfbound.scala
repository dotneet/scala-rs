trait Root[A]
class Base[A,C <: Root[A]] extends Root[A]
class Child extends Base[Int,Child]
object Main { def main(args:Array[String]):Unit=println(new Child!=null) }