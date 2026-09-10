trait H { val f:()=>Int }; object Main {def main(args:Array[String]):Unit={val build=(s:Int)=>new H{val f=()=>s};println(build(7).f())}}
