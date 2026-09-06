abstract class Provider {
  def id[U <: Singleton](value: U): U
}
