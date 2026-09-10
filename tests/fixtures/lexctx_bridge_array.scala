trait Box {type T;val value:T};object N extends Box {type T=Array[Int];val value=Array(7)};object Main {def main(args:Array[String]):Unit={val b:Box{type T=Array[Int]}=N;println(b.value(0))}}
