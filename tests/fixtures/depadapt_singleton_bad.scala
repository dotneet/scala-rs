class Box; object Main { def id[A <: AnyRef](a:A):a.type=a;val b=new Box;val c=new Box;val wrong:c.type=id(b) }
