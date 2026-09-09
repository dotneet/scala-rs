package scala.collection { trait BitSet; object BitSet { class SerializationProxy(val coll: BitSet) { protected var elems: Int = 1 } } }
package scala.collection.immutable {
 class BitSet extends scala.collection.BitSet
 object BitSet {
  def fromBitMaskNoCopy(elems: Int): BitSet = new BitSet
  private final class SerializationProxy(coll: BitSet) extends scala.collection.BitSet.SerializationProxy(coll) {
   protected[this] def readResolve(): Any = BitSet.fromBitMaskNoCopy(elems)
  }
 }
}
