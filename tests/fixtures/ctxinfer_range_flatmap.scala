object Main {
  def main(args: Array[String]): Unit = {
    val typed: Seq[(Int, Short)] = (0 until 4).flatMap { col =>
      if (col % 2 == 0) Some(col -> col.toShort) else None
    }
    val inferred = (0 until 4).flatMap { col =>
      if (col % 2 == 0) Some(col -> col.toShort) else None
    }
    val inferredTyped: IndexedSeq[(Int, Short)] = inferred
    val explicit = (0 until 4).flatMap[(Int, Short)] { col =>
      if (col % 2 == 0) Some(col -> col.toShort) else None
    }
    println(typed.mkString(",") + "|" + inferredTyped.mkString(",") + "|" + explicit.mkString(","))
  }
}
