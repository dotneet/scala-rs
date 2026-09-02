package mism13.util

/** slick's `slick.util.DumpInfo`, reduced. */
case class DumpInfo(name: String, mainInfo: String = "", attrInfo: String = "") {
  def namePlusMainInfo: String =
    if (name.nonEmpty && mainInfo.nonEmpty) name + " " + mainInfo else name + mainInfo
}

trait Dumpable {
  def getDumpInfo: DumpInfo
}
