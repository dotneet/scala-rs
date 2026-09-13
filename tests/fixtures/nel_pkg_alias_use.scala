package nelpkguse

import nelpkg.data._

object Main {
  val value: NonEmptyLazyList[Int] = null
  val checked = identity[NonEmptyLazyList[Int]](value)
}
