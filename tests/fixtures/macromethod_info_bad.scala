object Main { val inferred = MethodInfo.inferred[Recursive] }
class Recursive { private def hidden = hidden }
