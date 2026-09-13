class ProcessTreeLib {
  def processTree[T](git: Int, id: Int)(f: (String, Int) => T): T = f("tree", id)
}
