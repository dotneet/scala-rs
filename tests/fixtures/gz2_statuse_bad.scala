// A companion's members are not inherited by a subclass of the companion's
// class, however the class file spells them. scalac: "not found: value mk" /
// "not found: value tag".
import gz2lib.Gz2Base

class Gz2Bad extends Gz2Base(1) {
  def no: String = mk(3)
  def noVal: String = tag
}
