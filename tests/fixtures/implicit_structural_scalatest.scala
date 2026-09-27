package stt

import org.scalatest.funsuite.AnyFunSuite
import org.scalatest.matchers.should.Matchers

// `have length`, `have size` and `be empty` each need one `Length`, `Size`
// or `Emptiness` instance out of candidates over collection constructors,
// Java collections and structural members.
class LenSuite extends AnyFunSuite with Matchers {
  test("length") {
    val xs = List(1, 2, 3)
    xs should have length 3
    xs should not be empty
    "abc" should have length 3
    Vector(1) should have size 1
    Map(1 -> 2) should not be empty
    Array(1, 2) should have length 2
    Option(1) should not be empty
    "" shouldBe empty
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val s = new LenSuite
    println(s.testNames.mkString(","))
    org.scalatest.run(s)
  }
}
