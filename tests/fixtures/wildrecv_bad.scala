object WildImmutable { val fixed: Int = 1 }
object WildBad { def run(): Unit = { import WildImmutable._; fixed = 2 } }
