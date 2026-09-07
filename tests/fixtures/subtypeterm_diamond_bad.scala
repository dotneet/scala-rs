// The termination guard for `SymbolTable::is_sub_type`.
//
// 22 levels of diamonds: `A(n)` and `B(n)` each extend both `A(n-1)` and
// `B(n-1)`, so there are 2^22 distinct paths from `A22` to the top. The
// hierarchy is legal and *acyclic* -- scalac 2.13.16 compiles this file and
// reports the one type mismatch below in about two seconds.
//
// The walk in `is_sub_type` used to take every one of those paths, because it
// was bounded by depth alone and a depth bound bounds the depth of the
// recursion tree, not its size. `any()` short-circuits only on `true`, and the
// answer here is `false`, so nothing cut it short: this file took 4 s before
// the fix, and one level added doubled it (26 levels took 74 s).
//
// The right-hand side has to be something other than a class for the walk to
// run at all -- the class-against-class "no" is already answered linearly by
// `class_reaches` -- so the question asked is `A22 <: Double`.
trait A0
trait B0
trait A1 extends A0 with B0
trait B1 extends A0 with B0
trait A2 extends A1 with B1
trait B2 extends A1 with B1
trait A3 extends A2 with B2
trait B3 extends A2 with B2
trait A4 extends A3 with B3
trait B4 extends A3 with B3
trait A5 extends A4 with B4
trait B5 extends A4 with B4
trait A6 extends A5 with B5
trait B6 extends A5 with B5
trait A7 extends A6 with B6
trait B7 extends A6 with B6
trait A8 extends A7 with B7
trait B8 extends A7 with B7
trait A9 extends A8 with B8
trait B9 extends A8 with B8
trait A10 extends A9 with B9
trait B10 extends A9 with B9
trait A11 extends A10 with B10
trait B11 extends A10 with B10
trait A12 extends A11 with B11
trait B12 extends A11 with B11
trait A13 extends A12 with B12
trait B13 extends A12 with B12
trait A14 extends A13 with B13
trait B14 extends A13 with B13
trait A15 extends A14 with B14
trait B15 extends A14 with B14
trait A16 extends A15 with B15
trait B16 extends A15 with B15
trait A17 extends A16 with B16
trait B17 extends A16 with B16
trait A18 extends A17 with B17
trait B18 extends A17 with B17
trait A19 extends A18 with B18
trait B19 extends A18 with B18
trait A20 extends A19 with B19
trait B20 extends A19 with B19
trait A21 extends A20 with B20
trait B21 extends A20 with B20
trait A22 extends A21 with B21
trait B22 extends A21 with B21

object Main {
  def main(args: Array[String]): Unit = {
    val x: A22 = null
    val d: Double = x
    println(d)
  }
}
