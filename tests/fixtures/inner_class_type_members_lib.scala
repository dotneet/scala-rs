package icl

trait CompA { type KA; def mk: KA; final class InnerA(val k: KA) { def get: KA = k }; def mkI(i: KA): InnerA = new InnerA(i) }
