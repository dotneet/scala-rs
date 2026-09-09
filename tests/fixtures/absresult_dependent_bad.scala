trait Space { type T; val x: T }
trait Extractor { def extract(s: Space, other: Space): s.T }
class Sub extends Extractor { def extract(s: Space, other: Space) = other.x }
