object Main {
 trait Cmp[T] { def compare(a:T,b:T):Int }
 def sort[T <: java.lang.Number](x:Cmp[_ >: T]):Unit=()
 sort[java.lang.Integer]((a:String,b:String)=>a.length-b.length)
}
