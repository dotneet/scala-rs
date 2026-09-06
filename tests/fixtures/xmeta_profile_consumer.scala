import xmetaprofile.ConcreteProfile

object Main {
  val schema: ConcreteProfile.api.Item[String] = ConcreteProfile.api.one

  def main(args: Array[String]): Unit = {
    println(schema.++(schema).getClass.getName)
  }
}
