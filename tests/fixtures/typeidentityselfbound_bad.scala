trait Root[A]
class Base[A,C <: Root[A]] extends Root[A]
class Child extends Base[Int,String]