trait P {type T;val value:T};trait Q extends P {type T=Int;def read:Int=value};class N extends Q {val value=7};object Main {def main(args:Array[String]):Unit=println(new N().read)}
