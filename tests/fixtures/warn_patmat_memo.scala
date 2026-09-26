// `warn_patmat_order.scala` with a repeated exhaustive match and a repeated
// match with an unreachable case before every method. The repeats are
// answered from the analysis memo (`warn_patmat::AnalysisMemo`): each one
// still reports its unreachable case, and the order-sensitive matches in
// between still name scalac's counter-examples.
sealed trait Op
case object O0 extends Op
case object O1 extends Op
case object O2 extends Op
case object O3 extends Op
case object O4 extends Op
case object O5 extends Op
case object O6 extends Op
case object O7 extends Op
case object O8 extends Op
case object O9 extends Op
case object O10 extends Op
case object O11 extends Op
case object O12 extends Op
case object O13 extends Op
case object O14 extends Op
case object O15 extends Op
case object O16 extends Op
case object O17 extends Op
case object O18 extends Op
case object O19 extends Op
case object O20 extends Op
case object O21 extends Op
case object O22 extends Op
case object O23 extends Op
case object O24 extends Op
case object O25 extends Op
case object O26 extends Op
case object O27 extends Op
case object O28 extends Op
case object O29 extends Op
case object O30 extends Op
case object O31 extends Op
case object O32 extends Op
case object O33 extends Op
case object O34 extends Op
case object O35 extends Op
case object O36 extends Op
case object O37 extends Op
case object O38 extends Op
case object O39 extends Op
final case class Custom(n: Int, o: Op) extends Op
final case class Pair(a: Op, b: Op) extends Op
sealed trait T; final case class A(x: Option[Int]) extends T; final case class B(y: Either[String, Op]) extends T; case object C extends T

object W {
  def h0(o: Op, t: T): Int = (o, t) match {
    case (O1, A(Some(_))) => 1
    case (Custom(_, O2), _) => 2
    case (_, B(Left(_))) => 3
    case _ => 0
  }
  def u0(o: Op): Int = o match {
    case O1 | O2 => 1
    case O1 => 2
    case _ => 3
  }
  def m2(o: Op, t: T): Int = (o, t) match {
    case (O26, _) => 26
    case (O10, _) => 10
    case (O21, _) => 21
    case (O9, _) => 9
    case (O31, _) => 31
    case (O39, _) => 39
    case (O2, _) => 2
    case (O4, _) => 4
    case (O20, _) => 20
    case (O38, _) => 38
    case (O22, _) => 22
    case (O11, _) => 11
    case (O19, _) => 19
    case (O15, _) => 15
    case (O18, _) => 18
    case (O14, _) => 14
    case (O33, _) => 33
    case (O23, _) => 23
    case (O8, _) => 8
    case (O34, _) => 34
    case (O29, _) => 29
    case (O1, _) => 1
    case (O36, _) => 36
    case (O24, _) => 24
    case (O17, _) => 17
    case (O28, _) => 28
    case (O6, _) => 6
    case (O30, _) => 30
    case (O5, _) => 5
    case (O0, _) => 0
    case (O7, _) => 7
    case (O16, _) => 16
    case (O27, _) => 27
    case (O32, _) => 32
    case (O12, _) => 12
    case (Custom(1, O1), A(Some(3))) => 7
    case (Pair(O2, _), B(Right(O3))) => 7
    case (_, A(None)) => 7
  }
  def h1(o: Op, t: T): Int = (o, t) match {
    case (O1, A(Some(_))) => 1
    case (Custom(_, O2), _) => 2
    case (_, B(Left(_))) => 3
    case _ => 0
  }
  def u1(o: Op): Int = o match {
    case O1 | O2 => 1
    case O1 => 2
    case _ => 3
  }
  def m7(o: Op, t: T): Int = (o, t) match {
    case (O30, _) => 30
    case (O7, _) => 7
    case (O38, _) => 38
    case (O31, _) => 31
    case (O29, _) => 29
    case (O39, _) => 39
    case (O34, _) => 34
    case (O19, _) => 19
    case (O5, _) => 5
    case (O4, _) => 4
    case (O3, _) => 3
    case (O23, _) => 23
    case (O10, _) => 10
    case (O28, _) => 28
    case (O8, _) => 8
    case (O15, _) => 15
    case (O22, _) => 22
    case (O36, _) => 36
    case (O16, _) => 16
    case (O0, _) => 0
    case (O6, _) => 6
    case (O21, _) => 21
    case (O11, _) => 11
    case (O33, _) => 33
    case (O20, _) => 20
    case (O12, _) => 12
    case (Custom(1, O1), A(Some(3))) => 7
    case (Custom(_, Pair(_, _)), C) => 7
  }
  def h2(o: Op, t: T): Int = (o, t) match {
    case (O1, A(Some(_))) => 1
    case (Custom(_, O2), _) => 2
    case (_, B(Left(_))) => 3
    case _ => 0
  }
  def u2(o: Op): Int = o match {
    case O1 | O2 => 1
    case O1 => 2
    case _ => 3
  }
  def m13(o: Op, t: T): Int = (o, t) match {
    case (O1, _) => 1
    case (O28, _) => 28
    case (O11, _) => 11
    case (O0, _) => 0
    case (O9, _) => 9
    case (O37, _) => 37
    case (O35, _) => 35
    case (O30, _) => 30
    case (O7, _) => 7
    case (O17, _) => 17
    case (O39, _) => 39
    case (O10, _) => 10
    case (O21, _) => 21
    case (O16, _) => 16
    case (O26, _) => 26
    case (O32, _) => 32
    case (O15, _) => 15
    case (O3, _) => 3
    case (O24, _) => 24
    case (O29, _) => 29
    case (O31, _) => 31
    case (O6, _) => 6
    case (O8, _) => 8
    case (O20, _) => 20
    case (O22, _) => 22
    case (O27, _) => 27
    case (O19, _) => 19
    case (O14, _) => 14
    case (O36, _) => 36
    case (O25, _) => 25
    case (O13, _) => 13
    case (O5, _) => 5
    case (O23, _) => 23
    case (O12, _) => 12
    case (O2, _) => 2
    case (Pair(Custom(_, _), O4), B(Left("x"))) => 7
    case (_, A(None)) => 7
    case (Custom(_, Pair(_, _)), C) => 7
  }
  def h3(o: Op, t: T): Int = (o, t) match {
    case (O1, A(Some(_))) => 1
    case (Custom(_, O2), _) => 2
    case (_, B(Left(_))) => 3
    case _ => 0
  }
  def u3(o: Op): Int = o match {
    case O1 | O2 => 1
    case O1 => 2
    case _ => 3
  }
  def m34(o: Op, t: T): Int = (o, t) match {
    case (O15, _) => 15
    case (O35, _) => 35
    case (O39, _) => 39
    case (O1, _) => 1
    case (O26, _) => 26
    case (O19, _) => 19
    case (O3, _) => 3
    case (O36, _) => 36
    case (O12, _) => 12
    case (O37, _) => 37
    case (O28, _) => 28
    case (O21, _) => 21
    case (O20, _) => 20
    case (O13, _) => 13
    case (O2, _) => 2
    case (O8, _) => 8
    case (O7, _) => 7
    case (O29, _) => 29
    case (O38, _) => 38
    case (O11, _) => 11
    case (O23, _) => 23
    case (O30, _) => 30
    case (O32, _) => 32
    case (O10, _) => 10
    case (O22, _) => 22
    case (O5, _) => 5
    case (O16, _) => 16
    case (O6, _) => 6
    case (O33, _) => 33
    case (O0, _) => 0
    case (O4, _) => 4
    case (O24, _) => 24
    case (O17, _) => 17
    case (O34, _) => 34
    case (O27, _) => 27
    case (O31, _) => 31
    case (Pair(O2, _), B(Right(O3))) => 7
    case (Pair(Custom(_, _), O4), B(Left("x"))) => 7
  }
}
object Main { def main(args: Array[String]): Unit = println("ok") }
