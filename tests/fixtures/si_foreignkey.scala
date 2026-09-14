import slick.jdbc.H2Profile.api._

class OptionShapeCategories(tag: Tag) extends Table[(Int, String)](tag, "categories") {
  def id = column[Int]("id", O.PrimaryKey)
  def name = column[String]("name")
  def * = (id, name)
}

object ForeignKeyShapeProbe {
  // Keep this fixture focused on foreignKey's signature; constructing a
  // TableQuery itself is a separate Slick macro that needs scala-reflect.
  val categories = null.asInstanceOf[TableQuery[OptionShapeCategories]]

  class Posts(tag: Tag) extends Table[(Int, String, Option[Int])](tag, "posts") {
    def id = column[Int]("id", O.PrimaryKey, O.AutoInc)
    def title = column[String]("title")
    def category = column[Option[Int]]("category")
    def * = (id, title, category)

    // This uses foreignKey's separate target-column and implicit Shape
    // clauses, plus its cross-clause default action getters.
    def categoryFK = foreignKey("category_fk", category, categories)(_.id.?)
    val query = categoryFK.map(_.id)
  }

  val posts = null.asInstanceOf[TableQuery[Posts]]
}
