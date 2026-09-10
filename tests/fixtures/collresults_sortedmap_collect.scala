import scala.collection.immutable.{TreeMap,SortedMap}
object Main {def f[K:Ordering,V](m:SortedMap[K,V]):SortedMap[K,V]=m.collect{case(k,v)=>(k,v)}
def main(args:Array[String]):Unit=println(f(TreeMap(2->"b",1->"a")))}
