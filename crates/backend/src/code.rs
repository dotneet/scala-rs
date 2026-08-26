//! Bytecode assembler with stack-depth tracking and backpatching jumps.

use crate::classfile::{encode_method_name, Code, ExceptionEntry, Pool};

#[derive(Clone, Copy, Debug)]
pub struct Label(pub usize);

pub struct Assembler {
    pub pool: Pool,
    pub bytes: Vec<u8>,
    pub stack: i32,
    pub max_stack: i32,
    pub max_locals: u16,
    patches: Vec<(usize, Label)>, // offset of u16 jump, label
    labels: Vec<Option<u16>>,
    exceptions: Vec<(Label, Label, Label, Option<String>)>,
}

impl Assembler {
    pub fn new(max_locals: u16) -> Self {
        Self::with_pool(Pool::new(), max_locals)
    }

    pub fn with_pool(pool: Pool, max_locals: u16) -> Self {
        Assembler {
            pool,
            bytes: Vec::new(),
            stack: 0,
            max_stack: 0,
            max_locals,
            patches: Vec::new(),
            labels: Vec::new(),
            exceptions: Vec::new(),
        }
    }

    fn bump(&mut self, d: i32) {
        self.stack += d;
        if self.stack < 0 {
            self.stack = 0;
        }
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
    }

    pub fn fresh_label(&mut self) -> Label {
        let i = self.labels.len();
        self.labels.push(None);
        Label(i)
    }

    pub fn mark(&mut self, l: Label) {
        self.labels[l.0] = Some(self.bytes.len() as u16);
    }

    /// Record a JVM exception-table entry. `end` is exclusive. `catch` is an
    /// internal class name, or `None` for catch-all (finally / any).
    pub fn exception(&mut self, start: Label, end: Label, handler: Label, catch: Option<&str>) {
        self.exceptions
            .push((start, end, handler, catch.map(str::to_string)));
    }

    /// Handler entry: the JVM pushes the exception object. Reset tracked stack.
    pub fn enter_handler(&mut self) {
        self.stack = 1;
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
    }

    fn emit_op(&mut self, op: u8) {
        self.bytes.push(op);
    }

    fn emit_u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }

    fn jump(&mut self, op: u8, l: Label, stack_delta: i32) {
        self.emit_op(op);
        self.patches.push((self.bytes.len(), l));
        self.emit_u16(0);
        self.bump(stack_delta);
    }

    pub fn nop(&mut self) {
        self.emit_op(0x00);
    }

    pub fn pop(&mut self) {
        self.emit_op(0x57);
        self.bump(-1);
    }

    pub fn pop2(&mut self) {
        self.emit_op(0x58);
        self.bump(-2);
    }

    pub fn dup(&mut self) {
        self.emit_op(0x59);
        self.bump(1);
    }

    /// `swap` (category-1 values only).
    pub fn swap(&mut self) {
        self.emit_op(0x5f);
    }

    /// `dup_x2` for three category-1 values: `a, b, c` → `c, a, b, c`.
    pub fn dup_x2(&mut self) {
        self.emit_op(0x5b);
        self.bump(1);
    }

    pub fn aconst_null(&mut self) {
        self.emit_op(0x01);
        self.bump(1);
    }

    pub fn iconst(&mut self, v: i32) {
        match v {
            -1 => self.emit_op(0x02),
            0 => self.emit_op(0x03),
            1 => self.emit_op(0x04),
            2 => self.emit_op(0x05),
            3 => self.emit_op(0x06),
            4 => self.emit_op(0x07),
            5 => self.emit_op(0x08),
            n if (i8::MIN as i32..=i8::MAX as i32).contains(&n) => {
                self.emit_op(0x10);
                self.bytes.push(n as u8);
            }
            n if (i16::MIN as i32..=i16::MAX as i32).contains(&n) => {
                self.emit_op(0x11);
                self.emit_u16(n as u16);
            }
            n => {
                let i = self.pool.integer(n);
                if i <= 255 {
                    self.emit_op(0x12);
                    self.bytes.push(i as u8);
                } else {
                    self.emit_op(0x13);
                    self.emit_u16(i);
                }
            }
        }
        self.bump(1);
    }

    pub fn lconst(&mut self, v: i64) {
        if v == 0 {
            self.emit_op(0x09);
        } else if v == 1 {
            self.emit_op(0x0a);
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&v) {
            self.iconst(v as i32);
            self.emit_op(0x85); // i2l
            self.bump(1);
            return;
        } else {
            let i = self.pool.long(v);
            self.emit_op(0x14); // ldc2_w
            self.emit_u16(i);
        }
        self.bump(2);
    }

    pub fn dconst(&mut self, v: f64) {
        if v.to_bits() == 0.0f64.to_bits() {
            self.emit_op(0x0e);
        } else if v.to_bits() == 1.0f64.to_bits() {
            self.emit_op(0x0f);
        } else {
            let i = self.pool.double(v);
            self.emit_op(0x14); // ldc2_w
            self.emit_u16(i);
        }
        self.bump(2);
    }

    pub fn ldc_string(&mut self, s: &str) {
        let i = self.pool.string(s);
        if i <= 255 {
            self.emit_op(0x12);
            self.bytes.push(i as u8);
        } else {
            self.emit_op(0x13);
            self.emit_u16(i);
        }
        self.bump(1);
    }

    pub fn iload(&mut self, n: u16) {
        self.load_store(0x1a, 0x15, n, 1);
        self.ensure_local(n);
    }
    pub fn lload(&mut self, n: u16) {
        self.load_store(0x1e, 0x16, n, 2);
        self.ensure_local(n + 1);
    }
    pub fn dload(&mut self, n: u16) {
        self.load_store(0x26, 0x18, n, 2);
        self.ensure_local(n + 1);
    }
    pub fn aload(&mut self, n: u16) {
        self.load_store(0x2a, 0x19, n, 1);
        self.ensure_local(n);
    }
    pub fn istore(&mut self, n: u16) {
        self.load_store(0x3b, 0x36, n, -1);
        self.ensure_local(n);
    }
    pub fn lstore(&mut self, n: u16) {
        self.load_store(0x3f, 0x37, n, -2);
        self.ensure_local(n + 1);
    }
    pub fn dstore(&mut self, n: u16) {
        self.load_store(0x47, 0x39, n, -2);
        self.ensure_local(n + 1);
    }
    pub fn astore(&mut self, n: u16) {
        self.load_store(0x4b, 0x3a, n, -1);
        self.ensure_local(n);
    }

    fn load_store(&mut self, op0: u8, opg: u8, n: u16, delta: i32) {
        if n < 4 {
            self.emit_op(op0 + n as u8);
        } else if n <= 255 {
            self.emit_op(opg);
            self.bytes.push(n as u8);
        } else {
            self.emit_op(0xc4); // wide
            self.emit_op(opg);
            self.emit_u16(n);
        }
        self.bump(delta);
    }

    fn ensure_local(&mut self, n: u16) {
        if n + 1 > self.max_locals {
            self.max_locals = n + 1;
        }
    }

    pub fn iadd(&mut self) {
        self.emit_op(0x60);
        self.bump(-1);
    }
    pub fn isub(&mut self) {
        self.emit_op(0x64);
        self.bump(-1);
    }
    pub fn imul(&mut self) {
        self.emit_op(0x68);
        self.bump(-1);
    }
    pub fn idiv(&mut self) {
        self.emit_op(0x6c);
        self.bump(-1);
    }
    pub fn irem(&mut self) {
        self.emit_op(0x70);
        self.bump(-1);
    }
    pub fn ineg(&mut self) {
        self.emit_op(0x74);
    }
    pub fn iand(&mut self) {
        self.emit_op(0x7e);
        self.bump(-1);
    }
    pub fn ior(&mut self) {
        self.emit_op(0x80);
        self.bump(-1);
    }
    pub fn ixor(&mut self) {
        self.emit_op(0x82);
        self.bump(-1);
    }
    pub fn ishl(&mut self) {
        self.emit_op(0x78);
        self.bump(-1);
    }
    pub fn ishr(&mut self) {
        self.emit_op(0x7a);
        self.bump(-1);
    }
    pub fn iushr(&mut self) {
        self.emit_op(0x7c);
        self.bump(-1);
    }

    pub fn ladd(&mut self) {
        self.emit_op(0x61);
        self.bump(-2);
    }
    pub fn lsub(&mut self) {
        self.emit_op(0x65);
        self.bump(-2);
    }
    pub fn lmul(&mut self) {
        self.emit_op(0x69);
        self.bump(-2);
    }
    pub fn ldiv(&mut self) {
        self.emit_op(0x6d);
        self.bump(-2);
    }
    pub fn lneg(&mut self) {
        self.emit_op(0x75);
    }

    pub fn dadd(&mut self) {
        self.emit_op(0x63);
        self.bump(-2);
    }
    pub fn dsub(&mut self) {
        self.emit_op(0x67);
        self.bump(-2);
    }
    pub fn dmul(&mut self) {
        self.emit_op(0x6b);
        self.bump(-2);
    }
    pub fn ddiv(&mut self) {
        self.emit_op(0x6f);
        self.bump(-2);
    }
    pub fn dneg(&mut self) {
        self.emit_op(0x77);
    }

    pub fn i2l(&mut self) {
        self.emit_op(0x85);
        self.bump(1);
    }
    pub fn i2d(&mut self) {
        self.emit_op(0x87);
        self.bump(1);
    }
    pub fn l2d(&mut self) {
        self.emit_op(0x8a);
    }
    pub fn i2b(&mut self) {
        self.emit_op(0x91);
    }

    pub fn goto(&mut self, l: Label) {
        self.jump(0xa7, l, 0);
    }
    pub fn ifeq(&mut self, l: Label) {
        self.jump(0x99, l, -1);
    }
    pub fn ifne(&mut self, l: Label) {
        self.jump(0x9a, l, -1);
    }
    pub fn iflt(&mut self, l: Label) {
        self.jump(0x9b, l, -1);
    }
    pub fn ifge(&mut self, l: Label) {
        self.jump(0x9c, l, -1);
    }
    pub fn ifgt(&mut self, l: Label) {
        self.jump(0x9d, l, -1);
    }
    pub fn ifle(&mut self, l: Label) {
        self.jump(0x9e, l, -1);
    }
    pub fn if_icmpeq(&mut self, l: Label) {
        self.jump(0x9f, l, -2);
    }
    pub fn if_icmpne(&mut self, l: Label) {
        self.jump(0xa0, l, -2);
    }
    pub fn if_icmplt(&mut self, l: Label) {
        self.jump(0xa1, l, -2);
    }
    pub fn if_icmpge(&mut self, l: Label) {
        self.jump(0xa2, l, -2);
    }
    pub fn if_icmpgt(&mut self, l: Label) {
        self.jump(0xa3, l, -2);
    }
    pub fn if_icmple(&mut self, l: Label) {
        self.jump(0xa4, l, -2);
    }
    pub fn ifnull(&mut self, l: Label) {
        self.jump(0xc6, l, -1);
    }
    pub fn ifnonnull(&mut self, l: Label) {
        self.jump(0xc7, l, -1);
    }

    pub fn ireturn(&mut self) {
        self.emit_op(0xac);
        self.stack = 0;
    }
    pub fn lreturn(&mut self) {
        self.emit_op(0xad);
        self.stack = 0;
    }
    pub fn dreturn(&mut self) {
        self.emit_op(0xaf);
        self.stack = 0;
    }
    pub fn areturn(&mut self) {
        self.emit_op(0xb0);
        self.stack = 0;
    }
    pub fn vreturn(&mut self) {
        self.emit_op(0xb1);
        self.stack = 0;
    }

    pub fn getstatic(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb2);
        self.emit_u16(i);
        self.bump(slots(desc));
    }
    pub fn putstatic(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb3);
        self.emit_u16(i);
        self.bump(-slots(desc));
    }
    pub fn getfield(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb4);
        self.emit_u16(i);
        self.bump(-1 + slots(desc));
    }
    pub fn putfield(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb5);
        self.emit_u16(i);
        self.bump(-1 - slots(desc));
    }

    pub fn invokevirtual(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.methodref(owner, &name, desc);
        self.emit_op(0xb6);
        self.emit_u16(i);
        self.bump(invoke_delta(desc, true));
    }
    pub fn invokespecial(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.methodref(owner, &name, desc);
        self.emit_op(0xb7);
        self.emit_u16(i);
        self.bump(invoke_delta(desc, true));
    }
    pub fn invokestatic(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.methodref(owner, &name, desc);
        self.emit_op(0xb8);
        self.emit_u16(i);
        self.bump(invoke_delta(desc, false));
    }
    pub fn invokeinterface(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.iface_ref(owner, &name, desc);
        self.emit_op(0xb9);
        self.emit_u16(i);
        let n_args = 1 + count_params(desc);
        self.bytes.push(n_args as u8);
        self.bytes.push(0);
        self.bump(invoke_delta(desc, true));
    }

    pub fn new_obj(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xbb);
        self.emit_u16(i);
        self.bump(1);
    }

    pub fn checkcast(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xc0);
        self.emit_u16(i);
    }

    pub fn instanceof(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xc1);
        self.emit_u16(i);
        // object -> int, stack same
    }

    pub fn arraylength(&mut self) {
        self.emit_op(0xbe);
    }

    /// `anewarray class` — count → array ref.
    pub fn anewarray(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xbd);
        self.emit_u16(i);
    }

    /// `newarray atype` — count → primitive array. `T_INT` is 10.
    pub fn newarray(&mut self, atype: u8) {
        self.emit_op(0xbc);
        self.bytes.push(atype);
    }

    pub fn aastore(&mut self) {
        self.emit_op(0x53);
        self.bump(-3);
    }

    pub fn iastore(&mut self) {
        self.emit_op(0x4f);
        self.bump(-3);
    }

    pub fn athrow(&mut self) {
        self.emit_op(0xbf);
        self.stack = 0;
    }

    pub fn monitorenter(&mut self) {
        self.emit_op(0xc2);
        self.bump(-1);
    }

    pub fn monitorexit(&mut self) {
        self.emit_op(0xc3);
        self.bump(-1);
    }

    pub fn finish(mut self) -> (Code, Pool) {
        let copy = self.patches.clone();
        for (at, lab) in copy {
            let target = self.labels[lab.0].unwrap_or(0);
            let from = at as i32 - 1; // opcode position
            let rel = target as i32 - from;
            let b = (rel as i16).to_be_bytes();
            self.bytes[at] = b[0];
            self.bytes[at + 1] = b[1];
        }
        let mut exceptions = Vec::new();
        let pending = self.exceptions.clone();
        for (start, end, handler, catch) in pending {
            let start_pc = self.labels[start.0].unwrap_or(0);
            let end_pc = self.labels[end.0].unwrap_or(0);
            let handler_pc = self.labels[handler.0].unwrap_or(0);
            let catch_type = match catch {
                Some(c) => self.pool.class(&c),
                None => 0,
            };
            exceptions.push(ExceptionEntry {
                start_pc,
                end_pc,
                handler_pc,
                catch_type,
            });
        }
        let code = Code {
            max_stack: self.max_stack.max(1) as u16,
            max_locals: self.max_locals.max(1),
            bytes: self.bytes,
            exceptions,
        };
        (code, self.pool)
    }
}

fn slots(desc: &str) -> i32 {
    match desc.chars().next() {
        Some('J') | Some('D') => 2,
        Some('V') => 0,
        _ => 1,
    }
}

fn count_params(desc: &str) -> usize {
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut n = 0;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        n += 1;
        match c {
            'L' => while chars.next() != Some(';') {},
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.peek() == Some(&'L') {
                    while chars.next() != Some(';') {}
                } else {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    n
}

fn invoke_delta(desc: &str, has_this: bool) -> i32 {
    let params = param_slots(desc);
    let ret = desc.split_once(')').map(|(_, r)| slots(r)).unwrap_or(0);
    ret - params - if has_this { 1 } else { 0 }
}

fn param_slots(desc: &str) -> i32 {
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut n = 0i32;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'J' | 'D' => n += 2,
            'L' => {
                n += 1;
                while chars.next() != Some(';') {}
            }
            '[' => {
                n += 1;
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.peek() == Some(&'L') {
                    while chars.next() != Some(';') {}
                } else {
                    chars.next();
                }
            }
            _ => n += 1,
        }
    }
    n
}
