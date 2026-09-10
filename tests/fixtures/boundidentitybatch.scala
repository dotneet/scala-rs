class Upper[A <: Number]
class Lower[A >: String]
class FBound[A <: Comparable[A]]
class Dependent[A,B <: A]
object Main {
 type Narrow[A <: Number]=Upper[A]
 def keep[A <: Number](x:Upper[A]):Upper[A]=x
 def main(args:Array[String]):Unit={
  val upper:Narrow[java.lang.Integer]=keep(new Upper[java.lang.Integer])
  val lower:Lower[AnyRef]=new Lower[AnyRef]
  val recursive:FBound[String]=new FBound[String]
  val dependent:Dependent[Number,java.lang.Integer]=new Dependent[Number,java.lang.Integer]
  val existential:Upper[_ <: String]=null
  println(upper!=null);println(lower!=null);println(recursive!=null);println(dependent!=null)
  println(existential==null); println(IdentityExistential.recursive==null)
 }
}

class IdentityRecursive[A <: IdentityRecursive[A]]
object IdentityExistential { val recursive: IdentityRecursive[X] forSome { type X } = null }
