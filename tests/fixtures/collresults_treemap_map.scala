import scala.collection.immutable.{TreeMap,SortedMap}
object Main {def f[K:Ordering,V](m:TreeMap[K,V]):TreeMap[K,(V,Int)]=m.map{case(k,v)=>(k,(v,1))}
def main(args:Array[String]):Unit=println(f(TreeMap(2->"b",1->"a")))}
