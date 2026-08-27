package jprot

class Peer {
  def fromPeer(b: Base): Int = b.secret()
}

class Sub extends Base {
  def mine: Int = this.secret()
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Peer().fromPeer(new Base()))
    println(new Sub().mine)
    println(Base.secretStatic())
  }
}
