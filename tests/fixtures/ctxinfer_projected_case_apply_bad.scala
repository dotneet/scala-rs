trait Kind { type Data }
class TextKind extends Kind { type Data = String }
case class Linked[K <: Kind](kind: K)(val value: K#Data)
object Main {
  val wrong = Linked(new TextKind)(42)
}
