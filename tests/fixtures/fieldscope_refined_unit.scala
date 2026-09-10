trait P {type T;val value:T};object N extends P {type T=Unit;val value=()};object Main {def main(args:Array[String]):Unit={val p:P{type T=Unit}=N;println(p.value)}}
