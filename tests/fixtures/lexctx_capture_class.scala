trait H {val f:()=>Int}; class Factory{val build=(s:Int)=>new H{val f=()=>s}}; object Main {def main(args:Array[String]):Unit=println(new Factory().build(7).f())}
