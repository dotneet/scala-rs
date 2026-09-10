import scala.collection.immutable.ArraySeq
object Warm {def len(x:ArraySeq[Int]):Int=x.length;def plain(m:Map[Int,Int])=m.map{case(k,v)=>(k,v+1)}}
