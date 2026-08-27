package object enrich {
  implicit class Rich(n: Int) {
    def twice: Int = n * 2
  }
}
