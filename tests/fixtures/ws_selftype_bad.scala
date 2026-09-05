// The self type still has to be a real type, and it still offers only its own
// members. Dropping the *signature* pass's complaint about a self type must
// not drop the body pass's.
//
// Real scalac 2.13.16 reports all three.
import wsl._

trait Bad1 { self: Table[?] =>
  // `Table` has no such member; the class file cannot invent one.
  val x = noSuchColumn("X")
}

trait Bad2 { self: Missing =>
  def y: Int = 1
}

trait Outer2 { self: Table[?] =>
  // A self type in a *nested* template is where the signature pass's
  // diagnostic is dropped; a real error there is still reported.
  trait Inner2 { self: AlsoMissing =>
    def z: Int = 2
  }
}
