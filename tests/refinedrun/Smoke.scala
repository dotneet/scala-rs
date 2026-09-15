import eu.timepit.refined.types.time.Month
import eu.timepit.refined.types.string.NonEmptyString
import eu.timepit.refined.types.digests.MD5
import eu.timepit.refined.internal.Adjacent

object RefinedSmoke {
  def main(args: Array[String]): Unit = {
    assert(Month.from(12).isRight)
    assert(Month.from(0).isLeft)
    assert(Month.from(13).isLeft)
    assert(NonEmptyString.from("ok").isRight)
    assert(NonEmptyString.from("").isLeft)
    assert(MD5.from("d41d8cd98f00b204e9800998ecf8427e").isRight)
    assert(MD5.from("bad").isLeft)
    assert(MD5.from("z41d8cd98f00b204e9800998ecf8427e").isLeft)
    assert(Adjacent[Double].nextUp(1.0) > 1.0)
    assert(Adjacent[Float].nextDown(1.0f) < 1.0f)
    println("REFINED_SMOKE_PASS")
  }
}
