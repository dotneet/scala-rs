// `object X extends <a jar profile>`: the shape gitbucket's
// `DatabaseConfig.BlockingPostgresDriver` is written in.
//
// slick's profile cake leaves thirteen nested `object`s and a `val
// capabilities` abstract *in the class file* -- a trait's `val` is assigned
// through a synthetic `pkg$Owner$_setter_$x_$eq` from its `$init$`, and a
// nested `object`'s accessor is the implementing class's to define -- so
// reading the JVM flag asked this object to implement fourteen members it
// inherits ("object creation impossible"). The backend owes the class a field,
// a getter and a mixin setter for each `val`, and a lazily initialised
// `N$module` field plus accessor for each nested `object`; the nested module's
// constructor parameter says which enclosing instance it wants, which for a
// cake component is its *self type* (`SelectPart$(JdbcProfile)`).
object GzeroProfileObj {
  object Pg extends slick.jdbc.PostgresProfile {
    override def quoteIdentifier(id: String): String = "\"" + id.toLowerCase + "\""
  }

  def main(args: Array[String]): Unit = {
    // A `def` the object overrides, reached through the trait's own code.
    println(Pg.quoteIdentifier("Abc"))
    // A trait `val` the trait's `$init$` assigns (`capabilities`).
    println(Pg.capabilities.nonEmpty)
    // Nested `object`s of three different components, each with its own
    // enclosing-instance type.
    println(Pg.Sequence.getClass.getName)
    println(Pg.DDL.getClass.getName)
    println(Pg.JdbcType.getClass.getName)
    // The same instance every time.
    println(Pg.Sequence eq Pg.Sequence)
  }
}
