class Consumer extends Provider {
  def id[U <: String with Singleton](value: U): U = value
}
