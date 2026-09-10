package classnames
class +(val x:Int)
class ++(val x:Int)
class Outer { class ++(val x:Int); def make(x:Int): ++ = new ++(x) }
class Api { def make(x:Int): ++ = new ++(x) }
