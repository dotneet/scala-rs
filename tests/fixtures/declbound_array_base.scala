class Arrays[T](val values:Array[T]) {
def strings:Array[String]=Array("ok");def ints:Array[Int]=Array(7);def generic:Array[T]=values
def nested:Array[Array[String]]=Array(Array("nested"));def empty():Array[Int]=Array(8)
def poly[A]:Array[String]=Array("poly");def existential:Array[_ <: CharSequence]=Array("exist")
}
