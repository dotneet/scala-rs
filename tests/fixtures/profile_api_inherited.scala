trait BasicProfileForInheritedApi {
  trait BasicAPI
  val api: BasicAPI
}

trait RelationalProfileForInheritedApi extends BasicProfileForInheritedApi {
  self: RelationalProfileForInheritedApi =>

  class Tag
  class ColumnOptions { val PrimaryKey: String = "primary" }

  trait RelationalAPI extends BasicAPI {
    type Tag = self.Tag
    type Table[T] = self.Table[T]
  }

  override val api: RelationalAPI

  class Table[T](tag: Any, name: String) {
    val O: ColumnOptions = new ColumnOptions
    def column[C](name: String, option: String): C = null.asInstanceOf[C]
  }
}

trait BasicProfileBoundDbForInheritedApi {
  type Profile <: BasicProfileForInheritedApi
  val profile: Profile
}

trait RelationalProfileBoundDbForInheritedApi extends BasicProfileBoundDbForInheritedApi {
  type Profile <: RelationalProfileForInheritedApi
}

trait InheritedProfileApiUse extends RelationalProfileBoundDbForInheritedApi {
  import profile.api._

  class T(tag: Tag) extends Table[Int](tag, "t") {
    def id = column[Int]("id", O.PrimaryKey)
  }
}
