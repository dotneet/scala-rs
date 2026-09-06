class NullArrays {
 def f(x: Array[Null]): Int = 1
 def f(x: Array[AnyRef]): Int = 2
}
class NothingArrays {
 def f(x: Array[Nothing]): Int = 1
 def f(x: Array[AnyRef]): Int = 2
}
