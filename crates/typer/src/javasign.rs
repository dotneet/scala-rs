//! JVMS 4.7.9.1 Java `Signature` attribute parser (class / method).

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JType {
    Void,
    Boolean,
    Byte,
    Short,
    Char,
    Int,
    Long,
    Float,
    Double,
    Var(String),
    Array(Box<JType>),
    Class {
        jvm: String,
        args: Vec<JType>,
    },
    /// Unbounded wildcard `*`.
    Star,
    /// `+T` (`? extends T`).
    Extends(Box<JType>),
    /// `-T` (`? super T`).
    Super(Box<JType>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JParam {
    pub name: String,
    pub bounds: Vec<JType>,
}

pub struct ClassSig {
    pub tparams: Vec<JParam>,
    pub supers: Vec<JType>,
}

pub struct MethodSig {
    pub tparams: Vec<JParam>,
    pub params: Vec<JType>,
    pub ret: JType,
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn new(s: &'a str) -> Self {
        P {
            s: s.as_bytes(),
            i: 0,
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.i += 1;
        Some(b)
    }
    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        let start = self.i;
        while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
    fn parse_tparams(&mut self) -> Option<Vec<JParam>> {
        if !self.eat(b'<') {
            return Some(Vec::new());
        }
        let mut names = Vec::new();
        while self.peek() != Some(b'>') {
            let n = self.ident()?;
            if !self.eat(b':') {
                return None;
            }
            let mut bounds = Vec::new();
            if matches!(self.peek(), Some(b'L' | b'T' | b'[')) {
                bounds.push(self.parse_ref()?);
            }
            while self.eat(b':') {
                bounds.push(self.parse_ref()?);
            }
            names.push(JParam { name: n, bounds });
        }
        if !self.eat(b'>') {
            return None;
        }
        Some(names)
    }
    fn parse_java_type(&mut self) -> Option<JType> {
        match self.peek()? {
            b'V' => {
                self.bump();
                Some(JType::Void)
            }
            b'Z' => {
                self.bump();
                Some(JType::Boolean)
            }
            b'B' => {
                self.bump();
                Some(JType::Byte)
            }
            b'C' => {
                self.bump();
                Some(JType::Char)
            }
            b'I' => {
                self.bump();
                Some(JType::Int)
            }
            b'J' => {
                self.bump();
                Some(JType::Long)
            }
            b'F' => {
                self.bump();
                Some(JType::Float)
            }
            b'D' => {
                self.bump();
                Some(JType::Double)
            }
            b'S' => {
                self.bump();
                Some(JType::Short)
            }
            _ => self.parse_ref(),
        }
    }
    fn parse_ref(&mut self) -> Option<JType> {
        match self.peek()? {
            b'T' => {
                self.bump();
                let n = self.ident()?;
                if !self.eat(b';') {
                    return None;
                }
                Some(JType::Var(n))
            }
            b'[' => {
                self.bump();
                Some(JType::Array(Box::new(self.parse_java_type()?)))
            }
            b'L' => self.parse_class(),
            b'*' => {
                self.bump();
                Some(JType::Star)
            }
            b'+' => {
                self.bump();
                Some(JType::Extends(Box::new(self.parse_ref()?)))
            }
            b'-' => {
                self.bump();
                Some(JType::Super(Box::new(self.parse_ref()?)))
            }
            _ => None,
        }
    }
    fn parse_class(&mut self) -> Option<JType> {
        if !self.eat(b'L') {
            return None;
        }
        let mut jvm = String::new();
        let mut args = Vec::new();
        loop {
            let part = self.ident()?;
            jvm.push_str(&part);
            match self.peek() {
                Some(b'/') => {
                    self.bump();
                    jvm.push('/');
                }
                Some(b'.') => {
                    self.bump();
                    jvm.push('$');
                }
                Some(b'<') => {
                    args = self.parse_type_args()?;
                    while self.eat(b'.') {
                        jvm.push('$');
                        jvm.push_str(&self.ident()?);
                        if self.peek() == Some(b'<') {
                            args = self.parse_type_args()?;
                        }
                    }
                    if !self.eat(b';') {
                        return None;
                    }
                    return Some(JType::Class { jvm, args });
                }
                Some(b';') => {
                    self.bump();
                    return Some(JType::Class { jvm, args });
                }
                _ => return None,
            }
        }
    }
    fn parse_type_args(&mut self) -> Option<Vec<JType>> {
        if !self.eat(b'<') {
            return None;
        }
        let mut args = Vec::new();
        while self.peek() != Some(b'>') {
            args.push(self.parse_type_arg()?);
        }
        if !self.eat(b'>') {
            return None;
        }
        Some(args)
    }
    fn parse_type_arg(&mut self) -> Option<JType> {
        match self.peek()? {
            b'*' => {
                self.bump();
                Some(JType::Star)
            }
            b'+' => {
                self.bump();
                Some(JType::Extends(Box::new(self.parse_ref()?)))
            }
            b'-' => {
                self.bump();
                Some(JType::Super(Box::new(self.parse_ref()?)))
            }
            _ => self.parse_ref(),
        }
    }
}

pub fn parse_class_sig(s: &str) -> Option<ClassSig> {
    let mut p = P::new(s);
    let tparams = p.parse_tparams()?;
    let mut supers = Vec::new();
    while p.peek().is_some() {
        supers.push(p.parse_class()?);
    }
    Some(ClassSig { tparams, supers })
}

pub fn parse_method_sig(s: &str) -> Option<MethodSig> {
    let mut p = P::new(s);
    let tparams = p.parse_tparams()?;
    if !p.eat(b'(') {
        return None;
    }
    let mut params = Vec::new();
    while p.peek() != Some(b')') {
        params.push(p.parse_java_type()?);
    }
    if !p.eat(b')') {
        return None;
    }
    let ret = p.parse_java_type()?;
    Some(MethodSig {
        tparams,
        params,
        ret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tparam_names(ps: &[JParam]) -> Vec<&str> {
        ps.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn class_sig_arraylist() {
        let s = parse_class_sig(
            "<E:Ljava/lang/Object;>Ljava/util/AbstractList<TE;>;Ljava/util/List<TE;>;Ljava/util/RandomAccess;Ljava/lang/Cloneable;Ljava/io/Serializable;",
        )
        .unwrap();
        assert_eq!(tparam_names(&s.tparams), vec!["E"]);
        assert!(s.supers.iter().any(|t| matches!(
            t,
            JType::Class { jvm, args } if jvm == "java/util/List" && args.len() == 1
        )));
    }

    #[test]
    fn method_sig_get_and_add() {
        let g = parse_method_sig("(I)TE;").unwrap();
        assert!(matches!(g.params[0], JType::Int));
        assert!(matches!(g.ret, JType::Var(ref n) if n == "E"));
        let a = parse_method_sig("(TE;)Z").unwrap();
        assert!(matches!(a.params[0], JType::Var(ref n) if n == "E"));
        assert!(matches!(a.ret, JType::Boolean));
    }

    #[test]
    fn method_sig_aslist() {
        let m = parse_method_sig("<T:Ljava/lang/Object;>([TT;)Ljava/util/List<TT;>;").unwrap();
        assert_eq!(tparam_names(&m.tparams), vec!["T"]);
        assert!(matches!(m.params[0], JType::Array(_)));
        assert!(matches!(
            m.ret,
            JType::Class { ref jvm, ref args } if jvm == "java/util/List" && args.len() == 1
        ));
    }

    #[test]
    fn class_sig_map_entry() {
        let s = parse_class_sig("<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;")
            .unwrap();
        assert_eq!(tparam_names(&s.tparams), vec!["K", "V"]);
    }

    #[test]
    fn class_sig_simple_entry() {
        let s = parse_class_sig(
            "<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;Ljava/util/Map$Entry<TK;TV;>;Ljava/io/Serializable;",
        )
        .unwrap();
        assert_eq!(tparam_names(&s.tparams), vec!["K", "V"]);
        assert!(s.supers.iter().any(|t| matches!(
            t,
            JType::Class { jvm, args } if jvm == "java/util/Map$Entry" && args.len() == 2
        )));
    }

    #[test]
    fn method_sig_class_forname_wildcard() {
        let m = parse_method_sig("(Ljava/lang/String;)Ljava/lang/Class<*>;").unwrap();
        assert!(matches!(
            m.ret,
            JType::Class { ref jvm, ref args }
                if jvm == "java/lang/Class" && matches!(args.as_slice(), [JType::Star])
        ));
    }

    #[test]
    fn method_sig_collections_max_bounds_and_wildcard() {
        let m = parse_method_sig(
            "<T:Ljava/lang/Object;:Ljava/lang/Comparable<-TT;>;>(Ljava/util/Collection<+TT;>;)TT;",
        )
        .unwrap();
        assert_eq!(tparam_names(&m.tparams), vec!["T"]);
        assert_eq!(m.tparams[0].bounds.len(), 2);
        assert!(matches!(
            &m.tparams[0].bounds[1],
            JType::Class { jvm, args }
                if jvm == "java/lang/Comparable"
                    && matches!(args.as_slice(), [JType::Super(_)])
        ));
        assert!(matches!(
            &m.params[0],
            JType::Class { jvm, args }
                if jvm == "java/util/Collection"
                    && matches!(args.as_slice(), [JType::Extends(_)])
        ));
        assert!(matches!(m.ret, JType::Var(ref n) if n == "T"));
    }
}
