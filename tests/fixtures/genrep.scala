package genrep {
  package lifted {
    trait Rep[T] {
      def repString: String
      override def toString: String = repString
    }

    case class ConstRep[T](value: T) extends Rep[T] {
      def repString: String = "Rep(" + value + ")"
    }
  }

  package util {

    import scala.language.implicitConversions
    import genrep.lifted._

    // A class type parameter whose bound names an imported type. The namer
    // enters the parameter before the `import` above is processed, so the
    // bound can only be resolved when the class signature is typed.
    class Boxed[T <: Rep[_]](val rep: T) {
      def show: String = rep.toString
    }

    // The shape slick's generated `TupleSupport` uses: an `implicit class`
    // with bounded type parameters. The synthetic conversion has to carry
    // those parameters, or `RepOps` alone is a type constructor where a
    // proper type is required.
    //
    // `TupleOps2` is deliberately named so that a prefix test mistakes it for
    // `scala.TupleN`: slick's own `TupleShape[L, M, U, P]` used to be read as
    // a 4-tuple.
    object TupleMethods {
      implicit class RepOps[T <: Rep[_]](val c: T) {
        def ~[U <: Rep[_]](c2: U): (T, U) = (c, c2)
      }

      implicit class TupleOps2[T1 <: Rep[_], T2 <: Rep[_]](val t: (T1, T2)) {
        def ~[U <: Rep[_]](c: U): (T1, T2, U) = (t._1, t._2, c)
      }
    }
  }
}

object Main {
  import genrep.lifted._
  import genrep.util._
  import genrep.util.TupleMethods._

  // Every tuple is a `Product`, at the arities the prelude only knows from
  // the jar as well as the two it builds by hand.
  def buildTuple(s: scala.collection.Seq[Any]): Product = s.length match {
    case 1 => new Tuple1(s(0))
    case 2 => new Tuple2(s(0), s(1))
    case 3 => new Tuple3(s(0), s(1), s(2))
    case 4 => new Tuple4(s(0), s(1), s(2), s(3))
    case 5 => new Tuple5(s(0), s(1), s(2), s(3), s(4))
    case _ => new Tuple1(s.length)
  }

  // nsc packs an argument list that fits no alternative into one tuple:
  // `Some(a, b)` is `Some((a, b))`.
  def unapply3[T1, T2, T3](p: (T1, T2, T3)): Option[((T1, T2), T3)] =
    Some((p._1, p._2), p._3)

  // A *declared* tuple type, not one inferred from a `new TupleN`.
  def pairOf(a: Int, b: String): (Int, String) = (a, b)

  // slick's `class SetTupleParameter[-T <: Product](val children: SetParameter[_]*)`:
  // a repeated constructor parameter matches every argument against its
  // *element* type, and `Sink[T1]` conforms to `Sink[_]` even though `Sink`
  // is contravariant.
  class Sink[-T](val label: String)
  class Sinks(val parts: Sink[_]*) {
    val name: String = "sinks"
  }
  def mkSinks[T1, T2](s1: Sink[T1], s2: Sink[T2]): Sinks = new Sinks(s1, s2)

  def main(args: Array[String]): Unit = {
    val a = ConstRep(1)
    val b = ConstRep("x")
    val c = ConstRep(true)

    println(new Boxed(a).show)
    println(a ~ b)
    println((a, b) ~ c)

    // `s(0)` on `scala.collection.Seq`: `SeqOps.apply(Int)` and the inherited
    // `PartialFunction.apply` are one member, not an ambiguous overload.
    println(buildTuple(List[Any](1)))
    println(buildTuple(List[Any](1, 2)))
    println(buildTuple(List[Any](1, 2, 3)))
    println(buildTuple(List[Any](1, 2, 3, 4)))
    println(buildTuple(List[Any](1, 2, 3, 4, 5)))
    println(buildTuple(List[Any](1, 2, 3, 4, 5, 6)))

    val p4: Product = new Tuple4(1, 2, 3, 4)
    println(p4.productArity)
    val p2: Product = (1, "x")
    println(p2.productArity)
    val p3: Product = pairOf(1, "y")
    println(p3.productArity)

    println(unapply3((1, "x", true)))
    val o: Option[(Int, String)] = Some(1, "x")
    println(o)

    println(mkSinks(new Sink[Int]("i"), new Sink[String]("s")).name)
  }
}
