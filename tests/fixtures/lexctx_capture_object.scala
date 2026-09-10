trait H {val f:()=>Int}; object Main {val build=(s:Int)=>new H{val f=()=>s}; def main(args:Array[String]):Unit=println(build(7).f())}
