import testkit2lib.Profile.api._

object Main {
  // `Tag` is a nullary alias reached through the wildcard import. Before the
  // fix it stayed an unresolved name, so it matched no constructor parameter
  // ("type mismatch; found: Tag required: Tag").
  class Users(t: Tag) extends Table[Int](t, "users") { // secondary constructor
    // Unqualified names inherited from the binary parent -- the shape every
    // slick table body is written in (`column[Int]("id", O.PrimaryKey)`).
    def label: String = tableName + ":" + O.PrimaryKey
    def restated: String = describe
  }
  class Posts(t: Tag) extends Table[Int](t, Some("s"), "posts") // primary

  def main(args: Array[String]): Unit = {
    val t = new testkit2lib.Tag("p0")
    // `describe` is declared by the binary parent, not by the subclass.
    println(new Users(t).describe)
    println(new Posts(t).describe)
    // `O` has the singleton type of a `val`; selecting through it needs that
    // type's widening.
    println(O.PrimaryKey)
    println(new Users(t).O.PrimaryKey)
    println(new Users(t).label)
    println(new Users(t).restated)
  }
}
