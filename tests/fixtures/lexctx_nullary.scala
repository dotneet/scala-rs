trait H { def f:()=>String }; object Main {def main(args:Array[String]):Unit={val build=(s:String)=>new H{def f=()=>s};println(build("value").f())}}
