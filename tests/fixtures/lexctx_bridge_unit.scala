trait Box {type T;val value:T};object N extends Box {type T=Unit;val value=()};object Main {def main(args:Array[String]):Unit={val b:Box=N;println(b.value)}}
