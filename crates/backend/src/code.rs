//! Bytecode assembler with stack-depth tracking, verification types, and
//! backpatching jumps. Emits StackMapTable full frames for Java 8 (major 52).

use crate::classfile::{encode_method_name, Code, ExceptionEntry, Pool, ACC_STATIC};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct Label(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
enum VType {
    Top,
    Integer,
    Float,
    Long,
    Double,
    Null,
    UninitializedThis,
    Uninitialized(u16),
    Object(String),
}

impl VType {
    fn is_wide(&self) -> bool {
        matches!(self, VType::Long | VType::Double)
    }

    fn slots(&self) -> i32 {
        if self.is_wide() {
            2
        } else if matches!(self, VType::Top) {
            1
        } else {
            1
        }
    }
}

pub struct Assembler {
    pub pool: Pool,
    pub bytes: Vec<u8>,
    pub stack: i32,
    pub max_stack: i32,
    pub max_locals: u16,
    patches: Vec<(usize, Label)>, // offset of u16 jump, label
    /// i32 jump offsets (tableswitch / lookupswitch). (field_offset, label, opcode_pc)
    patches_i32: Vec<(usize, Label, usize)>,
    labels: Vec<Option<u16>>,
    exceptions: Vec<(Label, Label, Label, Option<String>)>,
    vstack: Vec<VType>,
    vlocals: Vec<VType>,
    label_stack: Vec<Option<Vec<VType>>>,
    label_locals: Vec<Option<Vec<VType>>>,
    frames: BTreeMap<u16, (Vec<VType>, Vec<VType>)>,
    dead: bool,
    need_frame: bool,
    this_name: String,
    is_init: bool,
    ret_object: Option<String>,
    /// Locals at the start of a try, used so exception handlers do not claim
    /// locals the try body initialized later.
    try_locals: Option<Vec<VType>>,
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
            patches_i32: Vec::new(),
            labels: Vec::new(),
            exceptions: Vec::new(),
            vstack: Vec::new(),
            vlocals: vec![VType::Top; max_locals as usize],
            label_stack: Vec::new(),
            label_locals: Vec::new(),
            frames: BTreeMap::new(),
            dead: false,
            need_frame: false,
            this_name: String::new(),
            is_init: false,
            ret_object: None,
            try_locals: None,
        }
    }

    /// Seed locals from the method descriptor so StackMapTable frames are valid.
    pub fn init_method(&mut self, access: u16, name: &str, desc: &str, this_name: &str) {
        self.this_name = this_name.to_string();
        self.is_init = name == "<init>";
        let is_static = access & ACC_STATIC != 0;
        if self.vlocals.is_empty() {
            self.vlocals.push(VType::Top);
        }
        let mut i = 0usize;
        if !is_static {
            self.set_local(
                0,
                if self.is_init {
                    VType::UninitializedThis
                } else {
                    VType::Object(this_name.to_string())
                },
            );
            i = 1;
        }
        for d in param_descs(desc) {
            let vt = vtype_from_desc(&d);
            let wide = vt.is_wide();
            self.set_local(i as u16, vt);
            i += 1;
            if wide {
                self.set_local(i as u16, VType::Top);
                i += 1;
            }
        }
        self.max_locals = self.max_locals.max(i as u16);
        let ret = desc.split_once(')').map(|(_, r)| r).unwrap_or("V");
        self.ret_object = match vtype_from_desc(ret) {
            VType::Object(s) => Some(s),
            _ => None,
        };
    }

    fn set_local(&mut self, n: u16, t: VType) {
        let i = n as usize;
        if i >= self.vlocals.len() {
            self.vlocals.resize(i + 1, VType::Top);
        }
        self.vlocals[i] = t;
        self.ensure_local(n);
    }

    fn push_v(&mut self, t: VType) {
        let s = t.slots();
        self.vstack.push(t);
        self.bump(s);
    }

    fn pop_v(&mut self) -> VType {
        if let Some(t) = self.vstack.pop() {
            self.bump(-t.slots());
            t
        } else {
            self.bump(-1);
            VType::Top
        }
    }

    fn pop_n_v(&mut self, n: usize) {
        for _ in 0..n {
            let _ = self.pop_v();
        }
    }

    fn record_frame_at(&mut self, off: u16, stack: Vec<VType>, locals: Vec<VType>) {
        if off == 0 {
            return;
        }
        self.frames.insert(off, (locals, stack));
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
        self.label_stack.push(None);
        self.label_locals.push(None);
        Label(i)
    }

    pub fn mark(&mut self, l: Label) {
        let off = self.bytes.len() as u16;
        self.labels[l.0] = Some(off);
        if self.dead {
            if let Some(st) = self.label_stack[l.0].clone() {
                self.vstack = st;
                self.stack = self.vstack.iter().map(|t| t.slots()).sum();
                self.dead = false;
            }
            if let Some(loc) = self.label_locals[l.0].clone() {
                self.vlocals = loc;
            }
        } else {
            if let Some(st) = self.label_stack[l.0].clone() {
                self.vstack = merge_stack(&self.vstack, &st, self.ret_object.as_deref());
                self.stack = self.vstack.iter().map(|t| t.slots()).sum();
            }
            if let Some(loc) = self.label_locals[l.0].clone() {
                self.vlocals = merge_locals(&self.vlocals, &loc);
            }
        }
        if self.label_stack[l.0].is_some() || self.need_frame {
            self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
            self.need_frame = false;
        }
        // A later backward jump merges against this state; without it, a local
        // first assigned inside the loop body would appear in the loop head's
        // frame even though it is undefined on entry.
        if !self.dead {
            self.save_label(l);
        }
    }

    /// Record a JVM exception-table entry. `end` is exclusive. `catch` is an
    /// internal class name, or `None` for catch-all (finally / any).
    pub fn exception(&mut self, start: Label, end: Label, handler: Label, catch: Option<&str>) {
        self.exceptions
            .push((start, end, handler, catch.map(str::to_string)));
    }

    /// Snapshot locals for a following non-local-return exception handler.
    pub fn capture_try_locals(&mut self) {
        self.try_locals = Some(self.vlocals.clone());
    }

    /// Handler entry: the JVM pushes the exception object. Reset tracked stack.
    pub fn enter_handler(&mut self) {
        self.dead = false;
        self.vstack = vec![VType::Object("java/lang/Throwable".into())];
        self.stack = 1;
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
        let off = self.bytes.len() as u16;
        self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
    }

    /// NLR catch: use locals from [`capture_try_locals`] so the handler frame
    /// does not claim locals the try body initialized later.
    pub fn enter_handler_captured_locals(&mut self) {
        self.dead = false;
        self.vstack = vec![VType::Object("java/lang/Throwable".into())];
        self.stack = 1;
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
        if let Some(loc) = self.try_locals.take() {
            self.vlocals = loc;
        }
        let off = self.bytes.len() as u16;
        self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
    }

    fn emit_op(&mut self, op: u8) {
        if self.need_frame {
            let off = self.bytes.len() as u16;
            self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
            self.need_frame = false;
        }
        self.bytes.push(op);
    }

    fn emit_u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }

    fn jump(&mut self, op: u8, l: Label, stack_delta: i32) {
        let n_pop = (-stack_delta).max(0) as usize;
        self.pop_n_v(n_pop);
        self.emit_op(op);
        self.patches.push((self.bytes.len(), l));
        self.emit_u16(0);
        self.save_label(l);
        if let Some(off) = self.labels[l.0] {
            if let (Some(st), Some(loc)) = (
                self.label_stack[l.0].clone(),
                self.label_locals[l.0].clone(),
            ) {
                self.record_frame_at(off, st, loc);
            }
        }
        if op == 0xa7 {
            self.dead = true;
            self.need_frame = true;
        }
        let _ = stack_delta;
    }

    fn save_label(&mut self, l: Label) {
        let stack = self.vstack.clone();
        let locals = self.vlocals.clone();
        self.label_stack[l.0] = Some(match self.label_stack[l.0].take() {
            Some(old) => merge_stack(&old, &stack, self.ret_object.as_deref()),
            None => stack,
        });
        self.label_locals[l.0] = Some(match self.label_locals[l.0].take() {
            Some(old) => merge_locals(&old, &locals),
            None => locals,
        });
    }

    pub fn nop(&mut self) {
        self.emit_op(0x00);
    }

    pub fn pop(&mut self) {
        self.emit_op(0x57);
        let _ = self.pop_v();
    }

    pub fn pop2(&mut self) {
        self.emit_op(0x58);
        let t = self.pop_v();
        if !t.is_wide() {
            let _ = self.pop_v();
        }
    }

    pub fn dup(&mut self) {
        self.emit_op(0x59);
        if let Some(t) = self.vstack.last().cloned() {
            self.push_v(t);
        } else {
            self.bump(1);
        }
    }

    /// `swap` (category-1 values only).
    pub fn swap(&mut self) {
        self.emit_op(0x5f);
        if self.vstack.len() >= 2 {
            let n = self.vstack.len();
            self.vstack.swap(n - 1, n - 2);
        }
    }

    /// `dup_x2`: three category-1 values `a, b, c` → `c, a, b, c`, or
    /// category-2 then category-1 `w, c` → `c, w, c`.
    pub fn dup_x2(&mut self) {
        self.emit_op(0x5b);
        let n = self.vstack.len();
        if n >= 2 {
            let top_wide = self.vstack[n - 1].is_wide();
            let next_wide = self.vstack[n - 2].is_wide();
            if !top_wide && next_wide {
                let c = self.vstack[n - 1].clone();
                self.vstack.insert(n - 2, c);
                self.bump(1);
                return;
            }
        }
        if n >= 3 {
            let c = self.vstack[n - 1].clone();
            self.vstack.insert(n - 3, c);
            self.bump(1);
        } else {
            self.bump(1);
        }
    }

    pub fn aconst_null(&mut self) {
        self.emit_op(0x01);
        self.push_v(VType::Null);
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
        self.push_v(VType::Integer);
    }

    pub fn lconst(&mut self, v: i64) {
        if v == 0 {
            self.emit_op(0x09);
        } else if v == 1 {
            self.emit_op(0x0a);
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&v) {
            self.iconst(v as i32);
            self.emit_op(0x85); // i2l
            let _ = self.pop_v();
            self.push_v(VType::Long);
            return;
        } else {
            let i = self.pool.long(v);
            self.emit_op(0x14); // ldc2_w
            self.emit_u16(i);
        }
        self.push_v(VType::Long);
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
        self.push_v(VType::Double);
    }

    pub fn fconst(&mut self, v: f32) {
        if v.to_bits() == 0.0f32.to_bits() {
            self.emit_op(0x0b);
        } else if v.to_bits() == 1.0f32.to_bits() {
            self.emit_op(0x0c);
        } else if v.to_bits() == 2.0f32.to_bits() {
            self.emit_op(0x0d);
        } else {
            let i = self.pool.float(v);
            if i <= 255 {
                self.emit_op(0x12); // ldc
                self.bytes.push(i as u8);
            } else {
                self.emit_op(0x13); // ldc_w
                self.emit_u16(i);
            }
        }
        self.push_v(VType::Float);
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
        self.push_v(VType::Object("java/lang/String".into()));
    }

    pub fn ldc_class(&mut self, internal: &str) {
        let i = self.pool.class(internal);
        if i <= 255 {
            self.emit_op(0x12);
            self.bytes.push(i as u8);
        } else {
            self.emit_op(0x13);
            self.emit_u16(i);
        }
        self.push_v(VType::Object("java/lang/Class".into()));
    }

    pub fn iload(&mut self, n: u16) {
        self.emit_load_store(0x1a, 0x15, n);
        self.push_v(VType::Integer);
        self.ensure_local(n);
    }
    pub fn lload(&mut self, n: u16) {
        self.emit_load_store(0x1e, 0x16, n);
        self.push_v(VType::Long);
        self.ensure_local(n + 1);
    }
    pub fn dload(&mut self, n: u16) {
        self.emit_load_store(0x26, 0x18, n);
        self.push_v(VType::Double);
        self.ensure_local(n + 1);
    }
    pub fn fload(&mut self, n: u16) {
        self.emit_load_store(0x22, 0x17, n);
        self.push_v(VType::Float);
        self.ensure_local(n);
    }
    pub fn aload(&mut self, n: u16) {
        self.emit_load_store(0x2a, 0x19, n);
        let t = self
            .vlocals
            .get(n as usize)
            .cloned()
            .unwrap_or_else(|| VType::Object("java/lang/Object".into()));
        let t = match t {
            VType::Top | VType::Integer | VType::Float | VType::Long | VType::Double => {
                VType::Object(self.this_name.clone())
            }
            other => other,
        };
        self.push_v(t);
        self.ensure_local(n);
    }
    pub fn istore(&mut self, n: u16) {
        let _ = self.pop_v();
        self.emit_load_store(0x3b, 0x36, n);
        self.set_local(n, VType::Integer);
    }
    pub fn lstore(&mut self, n: u16) {
        let _ = self.pop_v();
        self.emit_load_store(0x3f, 0x37, n);
        self.set_local(n, VType::Long);
        self.set_local(n + 1, VType::Top);
    }
    pub fn dstore(&mut self, n: u16) {
        let _ = self.pop_v();
        self.emit_load_store(0x47, 0x39, n);
        self.set_local(n, VType::Double);
        self.set_local(n + 1, VType::Top);
    }
    pub fn fstore(&mut self, n: u16) {
        let _ = self.pop_v();
        self.emit_load_store(0x43, 0x38, n);
        self.set_local(n, VType::Float);
    }
    pub fn astore(&mut self, n: u16) {
        let t = self.pop_v();
        self.emit_load_store(0x4b, 0x3a, n);
        self.set_local(n, t);
    }

    fn emit_load_store(&mut self, op0: u8, opg: u8, n: u16) {
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
    }

    #[allow(dead_code)]
    fn load_store(&mut self, op0: u8, opg: u8, n: u16, delta: i32) {
        self.emit_load_store(op0, opg, n);
        self.bump(delta);
    }

    fn ensure_local(&mut self, n: u16) {
        if n + 1 > self.max_locals {
            self.max_locals = n + 1;
        }
    }

    fn bin_int(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    fn bin_long(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Long);
    }
    fn bin_double(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Double);
    }

    pub fn iadd(&mut self) {
        self.bin_int(0x60);
    }
    pub fn isub(&mut self) {
        self.bin_int(0x64);
    }
    pub fn imul(&mut self) {
        self.bin_int(0x68);
    }
    pub fn idiv(&mut self) {
        self.bin_int(0x6c);
    }
    pub fn irem(&mut self) {
        self.bin_int(0x70);
    }
    pub fn ineg(&mut self) {
        self.emit_op(0x74);
    }
    pub fn iand(&mut self) {
        self.bin_int(0x7e);
    }
    pub fn ior(&mut self) {
        self.bin_int(0x80);
    }
    pub fn ixor(&mut self) {
        self.bin_int(0x82);
    }
    pub fn ishl(&mut self) {
        self.bin_int(0x78);
    }
    pub fn ishr(&mut self) {
        self.bin_int(0x7a);
    }
    pub fn iushr(&mut self) {
        self.bin_int(0x7c);
    }

    pub fn ladd(&mut self) {
        self.bin_long(0x61);
    }
    pub fn lsub(&mut self) {
        self.bin_long(0x65);
    }
    pub fn lmul(&mut self) {
        self.bin_long(0x69);
    }
    pub fn ldiv(&mut self) {
        self.bin_long(0x6d);
    }
    pub fn lneg(&mut self) {
        self.emit_op(0x75);
    }

    pub fn dadd(&mut self) {
        self.bin_double(0x63);
    }
    pub fn dsub(&mut self) {
        self.bin_double(0x67);
    }
    pub fn dmul(&mut self) {
        self.bin_double(0x6b);
    }
    pub fn ddiv(&mut self) {
        self.bin_double(0x6f);
    }
    pub fn dneg(&mut self) {
        self.emit_op(0x77);
    }
    pub fn fneg(&mut self) {
        self.emit_op(0x76);
    }

    pub fn i2l(&mut self) {
        self.emit_op(0x85);
        let _ = self.pop_v();
        self.push_v(VType::Long);
    }
    pub fn i2d(&mut self) {
        self.emit_op(0x87);
        let _ = self.pop_v();
        self.push_v(VType::Double);
    }
    pub fn l2d(&mut self) {
        self.emit_op(0x8a);
        let _ = self.pop_v();
        self.push_v(VType::Double);
    }
    /// `lcmp` / `fcmpl` / `dcmpl`: pop two values, push the comparison int.
    pub fn lcmp(&mut self) {
        self.emit_op(0x94);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn fcmpl(&mut self) {
        self.emit_op(0x95);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn dcmpl(&mut self) {
        self.emit_op(0x97);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn i2f(&mut self) {
        self.emit_op(0x86);
        let _ = self.pop_v();
        self.push_v(VType::Float);
    }
    pub fn l2f(&mut self) {
        self.emit_op(0x89);
        let _ = self.pop_v();
        self.push_v(VType::Float);
    }
    pub fn f2d(&mut self) {
        self.emit_op(0x8d);
        let _ = self.pop_v();
        self.push_v(VType::Double);
    }
    pub fn i2b(&mut self) {
        self.emit_op(0x91);
    }
    pub fn i2c(&mut self) {
        self.emit_op(0x92);
    }
    pub fn i2s(&mut self) {
        self.emit_op(0x93);
    }

    /// `dup_x1`: `…, v2, v1` → `…, v1, v2, v1` (category-1).
    pub fn dup_x1(&mut self) {
        self.emit_op(0x5a);
        if self.vstack.len() >= 2 {
            let v1 = self.vstack[self.vstack.len() - 1].clone();
            self.vstack.insert(self.vstack.len() - 2, v1);
            self.bump(1);
        } else {
            self.bump(1);
        }
    }

    pub fn goto(&mut self, l: Label) {
        self.jump(0xa7, l, 0);
    }

    /// `tableswitch`. `cases[i]` is the target for key `low + i`. Pops the int.
    pub fn tableswitch(&mut self, default: Label, low: i32, high: i32, cases: &[Label]) {
        let op_pc = self.bytes.len();
        self.emit_op(0xaa);
        let _ = self.pop_v();
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        self.emit_switch_offset(default, op_pc);
        self.emit_i32(low);
        self.emit_i32(high);
        for l in cases {
            self.emit_switch_offset(*l, op_pc);
        }
        self.save_label(default);
        for l in cases {
            self.save_label(*l);
        }
        self.kill();
    }

    /// `lookupswitch`. `pairs` must be sorted by match key. Pops the int.
    pub fn lookupswitch(&mut self, default: Label, pairs: &[(i32, Label)]) {
        let op_pc = self.bytes.len();
        self.emit_op(0xab);
        let _ = self.pop_v();
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        self.emit_switch_offset(default, op_pc);
        self.emit_i32(pairs.len() as i32);
        for (k, l) in pairs {
            self.emit_i32(*k);
            self.emit_switch_offset(*l, op_pc);
        }
        self.save_label(default);
        for (_, l) in pairs {
            self.save_label(*l);
        }
        self.kill();
    }

    fn emit_i32(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }

    fn emit_switch_offset(&mut self, l: Label, op_pc: usize) {
        self.patches_i32.push((self.bytes.len(), l, op_pc));
        self.emit_i32(0);
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
    pub fn if_acmpeq(&mut self, l: Label) {
        self.jump(0xa5, l, -2);
    }
    pub fn if_acmpne(&mut self, l: Label) {
        self.jump(0xa6, l, -2);
    }
    pub fn ifnull(&mut self, l: Label) {
        self.jump(0xc6, l, -1);
    }
    pub fn ifnonnull(&mut self, l: Label) {
        self.jump(0xc7, l, -1);
    }

    pub fn ireturn(&mut self) {
        self.emit_op(0xac);
        self.kill();
    }
    pub fn lreturn(&mut self) {
        self.emit_op(0xad);
        self.kill();
    }
    pub fn dreturn(&mut self) {
        self.emit_op(0xaf);
        self.kill();
    }
    pub fn freturn(&mut self) {
        self.emit_op(0xae);
        self.kill();
    }
    pub fn areturn(&mut self) {
        self.emit_op(0xb0);
        self.kill();
    }
    pub fn vreturn(&mut self) {
        self.emit_op(0xb1);
        self.kill();
    }

    fn kill(&mut self) {
        self.vstack.clear();
        self.stack = 0;
        self.dead = true;
        self.need_frame = true;
    }

    pub fn getstatic(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb2);
        self.emit_u16(i);
        self.push_v(vtype_from_desc(desc));
    }
    pub fn putstatic(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb3);
        self.emit_u16(i);
        let _ = self.pop_v();
    }
    pub fn getfield(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb4);
        self.emit_u16(i);
        let _ = self.pop_v();
        self.push_v(vtype_from_desc(desc));
    }
    pub fn putfield(&mut self, owner: &str, name: &str, desc: &str) {
        let i = self.pool.fieldref(owner, name, desc);
        self.emit_op(0xb5);
        self.emit_u16(i);
        let _ = self.pop_v();
        let _ = self.pop_v();
    }

    pub fn invokevirtual(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.methodref(owner, &name, desc);
        self.emit_op(0xb6);
        self.emit_u16(i);
        self.apply_invoke(desc, true, false, owner);
    }
    pub fn invokespecial(&mut self, owner: &str, name: &str, desc: &str) {
        let is_init = name == "<init>";
        let name = encode_method_name(name);
        let i = self.pool.methodref(owner, &name, desc);
        self.emit_op(0xb7);
        self.emit_u16(i);
        self.apply_invoke(desc, true, is_init, owner);
    }
    pub fn invokestatic(&mut self, owner: &str, name: &str, desc: &str) {
        self.invokestatic_ref(owner, name, desc, false);
    }
    pub fn invokestatic_interface(&mut self, owner: &str, name: &str, desc: &str) {
        self.invokestatic_ref(owner, name, desc, true);
    }
    fn invokestatic_ref(&mut self, owner: &str, name: &str, desc: &str, iface: bool) {
        let name = encode_method_name(name);
        let i = if iface {
            self.pool.iface_ref(owner, &name, desc)
        } else {
            self.pool.methodref(owner, &name, desc)
        };
        self.emit_op(0xb8);
        self.emit_u16(i);
        self.apply_invoke(desc, false, false, owner);
    }
    pub fn invokeinterface(&mut self, owner: &str, name: &str, desc: &str) {
        let name = encode_method_name(name);
        let i = self.pool.iface_ref(owner, &name, desc);
        self.emit_op(0xb9);
        self.emit_u16(i);
        let n_args = 1 + count_params(desc);
        self.bytes.push(n_args as u8);
        self.bytes.push(0);
        self.apply_invoke(desc, true, false, owner);
    }

    fn apply_invoke(&mut self, desc: &str, has_this: bool, is_init: bool, owner: &str) {
        let n_params = count_params(desc);
        self.pop_n_v(n_params);
        let recv = if has_this { Some(self.pop_v()) } else { None };
        if is_init {
            if let Some(recv) = recv {
                self.initialize(recv, owner);
            }
        }
        let ret = desc.split_once(')').map(|(_, r)| r).unwrap_or("V");
        if ret != "V" {
            self.push_v(vtype_from_desc(ret));
        }
    }

    fn initialize(&mut self, recv: VType, owner: &str) {
        let inited = VType::Object(owner.to_string());
        match recv {
            VType::UninitializedThis => {
                for t in self.vstack.iter_mut().chain(self.vlocals.iter_mut()) {
                    if matches!(t, VType::UninitializedThis) {
                        *t = inited.clone();
                    }
                }
            }
            VType::Uninitialized(off) => {
                for t in self.vstack.iter_mut().chain(self.vlocals.iter_mut()) {
                    if matches!(t, VType::Uninitialized(o) if *o == off) {
                        *t = inited.clone();
                    }
                }
            }
            _ => {}
        }
    }

    pub fn new_obj(&mut self, class: &str) {
        let off = self.bytes.len() as u16;
        let i = self.pool.class(class);
        self.emit_op(0xbb);
        self.emit_u16(i);
        self.push_v(VType::Uninitialized(off));
    }

    pub fn checkcast(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xc0);
        self.emit_u16(i);
        let _ = self.pop_v();
        self.push_v(VType::Object(class.to_string()));
    }

    pub fn instanceof(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xc1);
        self.emit_u16(i);
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }

    pub fn arraylength(&mut self) {
        self.emit_op(0xbe);
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }

    pub fn aaload(&mut self) {
        self.emit_op(0x32);
        let _ = self.pop_v();
        let arr = self.pop_v();
        let elem = match arr {
            VType::Object(s) if s.starts_with('[') => {
                let rest = &s[1..];
                vtype_from_desc(rest)
            }
            _ => VType::Object("java/lang/Object".into()),
        };
        self.push_v(elem);
    }

    pub fn iaload(&mut self) {
        self.emit_op(0x2e);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }

    /// `anewarray class` — count → array ref.
    pub fn anewarray(&mut self, class: &str) {
        let i = self.pool.class(class);
        self.emit_op(0xbd);
        self.emit_u16(i);
        let _ = self.pop_v();
        self.push_v(VType::Object(format!("[L{class};")));
    }

    /// `newarray atype` — count → primitive array. `T_INT` is 10.
    pub fn newarray(&mut self, atype: u8) {
        self.emit_op(0xbc);
        self.bytes.push(atype);
        let _ = self.pop_v();
        let tag = match atype {
            4 => "[Z",
            5 => "[C",
            6 => "[F",
            7 => "[D",
            8 => "[B",
            9 => "[S",
            10 => "[I",
            11 => "[J",
            _ => "[Ljava/lang/Object;",
        };
        self.push_v(VType::Object(tag.into()));
    }

    pub fn aastore(&mut self) {
        self.emit_op(0x53);
        let _ = self.pop_v();
        let _ = self.pop_v();
        let _ = self.pop_v();
    }

    pub fn iastore(&mut self) {
        self.emit_op(0x4f);
        let _ = self.pop_v();
        let _ = self.pop_v();
        let _ = self.pop_v();
    }

    pub fn athrow(&mut self) {
        self.emit_op(0xbf);
        self.kill();
    }

    pub fn monitorenter(&mut self) {
        self.emit_op(0xc2);
        let _ = self.pop_v();
    }

    pub fn monitorexit(&mut self) {
        self.emit_op(0xc3);
        let _ = self.pop_v();
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
        let copy32 = self.patches_i32.clone();
        for (at, lab, op_pc) in copy32 {
            let target = self.labels[lab.0].unwrap_or(0) as i32;
            let rel = target - op_pc as i32;
            let b = rel.to_be_bytes();
            self.bytes[at] = b[0];
            self.bytes[at + 1] = b[1];
            self.bytes[at + 2] = b[2];
            self.bytes[at + 3] = b[3];
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
        let stack_map = self.encode_stack_map();
        let code = Code {
            max_stack: self.max_stack.max(1) as u16,
            max_locals: self.max_locals.max(1),
            bytes: self.bytes,
            exceptions,
            stack_map,
        };
        (code, self.pool)
    }

    fn encode_stack_map(&mut self) -> Option<Vec<u8>> {
        let frames: Vec<(u16, Vec<VType>, Vec<VType>)> = self
            .frames
            .iter()
            .map(|(&off, (locals, stack))| (off, locals.clone(), stack.clone()))
            .collect();
        if frames.is_empty() {
            return None;
        }
        let mut body = Vec::new();
        body.extend_from_slice(&(frames.len() as u16).to_be_bytes());
        let mut prev: i32 = -1;
        for (off, locals, stack) in frames {
            let delta = if prev < 0 {
                off as i32
            } else {
                off as i32 - prev - 1
            };
            prev = off as i32;
            body.push(255u8); // full_frame
            body.extend_from_slice(&(delta as u16).to_be_bytes());
            let loc = compact_locals(&locals);
            body.extend_from_slice(&(loc.len() as u16).to_be_bytes());
            for t in &loc {
                self.write_vtype(&mut body, t);
            }
            body.extend_from_slice(&(stack.len() as u16).to_be_bytes());
            for t in &stack {
                self.write_vtype(&mut body, t);
            }
        }
        Some(body)
    }

    fn write_vtype(&mut self, out: &mut Vec<u8>, t: &VType) {
        match t {
            VType::Top => out.push(0),
            VType::Integer => out.push(1),
            VType::Float => out.push(2),
            VType::Double => out.push(3),
            VType::Long => out.push(4),
            VType::Null => out.push(5),
            VType::UninitializedThis => out.push(6),
            VType::Object(name) => {
                out.push(7);
                let i = self.pool.class(name);
                out.extend_from_slice(&i.to_be_bytes());
            }
            VType::Uninitialized(off) => {
                out.push(8);
                out.extend_from_slice(&off.to_be_bytes());
            }
        }
    }
}

fn vtype_from_desc(desc: &str) -> VType {
    match desc.chars().next() {
        Some('I') | Some('Z') | Some('B') | Some('C') | Some('S') => VType::Integer,
        Some('F') => VType::Float,
        Some('J') => VType::Long,
        Some('D') => VType::Double,
        Some('V') => VType::Top,
        Some('[') => VType::Object(desc.to_string()),
        Some('L') => {
            let inner = desc.trim_start_matches('L').trim_end_matches(';');
            VType::Object(inner.to_string())
        }
        _ => VType::Object("java/lang/Object".into()),
    }
}

fn param_descs(desc: &str) -> Vec<String> {
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut out = Vec::new();
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match b[i] {
            b'L' => {
                i += 1;
                while i < b.len() && b[i] != b';' {
                    i += 1;
                }
                if i < b.len() {
                    i += 1;
                }
            }
            b'[' => {
                i += 1;
                while i < b.len() && b[i] == b'[' {
                    i += 1;
                }
                if i < b.len() && b[i] == b'L' {
                    while i < b.len() && b[i] != b';' {
                        i += 1;
                    }
                    if i < b.len() {
                        i += 1;
                    }
                } else if i < b.len() {
                    i += 1;
                }
            }
            _ => i += 1,
        }
        out.push(inner[start..i].to_string());
    }
    out
}

fn merge_vtype(a: &VType, b: &VType, object_lub: Option<&str>) -> VType {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (VType::Null, VType::Object(s)) | (VType::Object(s), VType::Null) => {
            VType::Object(s.clone())
        }
        (VType::Object(_), VType::Object(_)) => {
            if let Some(lub) = object_lub {
                VType::Object(lub.to_string())
            } else {
                VType::Object("java/lang/Object".into())
            }
        }
        _ => VType::Top,
    }
}

fn merge_locals(a: &[VType], b: &[VType]) -> Vec<VType> {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or(VType::Top);
        let y = b.get(i).cloned().unwrap_or(VType::Top);
        out.push(merge_vtype(&x, &y, None));
    }
    out
}

fn merge_stack(a: &[VType], b: &[VType], object_lub: Option<&str>) -> Vec<VType> {
    if a.len() != b.len() {
        if a.len() < b.len() {
            return a.to_vec();
        }
        return b.to_vec();
    }
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| merge_vtype(x, y, object_lub))
        .collect()
}

fn compact_locals(slots: &[VType]) -> Vec<VType> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < slots.len() {
        let t = &slots[i];
        if matches!(t, VType::Top) {
            // keep holes except trailing (trimmed later)
            out.push(VType::Top);
            i += 1;
            continue;
        }
        out.push(t.clone());
        i += if t.is_wide() { 2 } else { 1 };
    }
    while out.last().is_some_and(|t| matches!(t, VType::Top)) {
        out.pop();
    }
    out
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
