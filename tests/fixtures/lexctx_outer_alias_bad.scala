trait AB {type T;trait Base {val value:T}}
object N extends AB {type T=Int;class C extends Base {val value="bad"}}
