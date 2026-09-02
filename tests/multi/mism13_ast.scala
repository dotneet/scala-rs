package mism13.ast

import mism13.util.{DumpInfo, Dumpable}

/** slick's `slick.ast.Node`: the base declares `getDumpInfo` with an inferred
  * result type, and `toString` reads it. */
trait Node extends Dumpable {
  def name: String
  def getDumpInfo = DumpInfo(name, "base")
  override final def toString = getDumpInfo.namePlusMainInfo
}
