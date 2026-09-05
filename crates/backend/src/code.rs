//! Bytecode assembler with stack-depth tracking, verification types, and
//! backpatching jumps. Emits StackMapTable full frames for Java 8 (major 52).

use crate::classfile::{encode_method_name, Code, ExceptionEntry, Pool, ACC_STATIC};
use std::collections::BTreeMap;

/// The furthest a 16-bit branch offset reaches, either way. Every branch
/// except `goto_w`/`jsr_w` carries one (JVMS 6.5).
const MIN_BRANCH: i64 = i16::MIN as i64;
const MAX_BRANCH: i64 = i16::MAX as i64;

#[derive(Clone, Copy, Debug)]
pub struct Label(pub usize);

/// One tracked operand-stack entry, described well enough for the generator to
/// spill it to a local and load it back. See [`Assembler::stack_entries`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackEntry {
    Int,
    Long,
    Float,
    Double,
    /// A reference. `Some(name)` is the tracked class, which the spill slot can
    /// then be declared to hold; `None` covers `null` and the still
    /// uninitialized result of a `new`, neither of which has a class to declare.
    Ref(Option<String>),
}

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
    /// The class the *top* of the stack merges to at this label, when the
    /// generator knows it. Branches of a `match` or an `if` push different
    /// classes (`scala/Some` and `scala/None$`); without this the merged frame
    /// says `java/lang/Object` and every use of the result that wants the
    /// static type -- `putfield`, a call's argument, `areturn` -- fails
    /// verification. The static type of the expression *is* the join, so the
    /// generator hands it over with [`Assembler::set_join_class`].
    label_join: Vec<Option<String>>,
    frames: BTreeMap<u16, (Vec<VType>, Vec<VType>)>,
    /// Verification state at the instruction *after* each conditional branch,
    /// in emission order.
    ///
    /// No conditional branch has a wide form, so one that cannot reach its
    /// label is rewritten as the inverse branch over a `goto_w` (see
    /// [`Assembler::widen_jumps`]). That makes the fall-through position a
    /// branch target, which JVMS 4.7.4 then needs a frame for -- and the state
    /// there is only known while the branch is being emitted. It is kept here
    /// and dropped in `finish` unless a rewrite actually happens.
    cond_frames: Vec<(u16, Vec<VType>, Vec<VType>)>,
    dead: bool,
    /// Byte offset just past the terminator that made the code dead. Everything
    /// emitted from here on is unreachable and is dropped again.
    dead_start: Option<usize>,
    need_frame: bool,
    this_name: String,
    is_init: bool,
    /// Stack of locals snapshots taken at the start of each open guarded region,
    /// so exception handlers do not claim locals the body initialized later.
    try_locals: Vec<Vec<VType>>,
    /// The class a local is *declared* to hold. `try`/`catch` parks its result
    /// in one, and the branches store different classes into it (`Success` and
    /// `Failure`); with no class hierarchy to take a least upper bound with,
    /// the merge would be `java/lang/Object` and the `areturn` that follows
    /// would fail verification. The static type of the expression is an upper
    /// bound of every branch, so the generator declares that instead.
    local_class: BTreeMap<u16, String>,
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
            label_join: Vec::new(),
            frames: BTreeMap::new(),
            cond_frames: Vec::new(),
            dead: false,
            dead_start: None,
            need_frame: false,
            this_name: String::new(),
            is_init: false,
            try_locals: Vec::new(),
            local_class: BTreeMap::new(),
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
    }

    /// Declare the class `slot` holds; see [`Assembler::local_class`].
    ///
    /// `java/lang/Object` counts: a `var a: Any` assigned an `Integer` before a
    /// loop and a `String` inside it needs *every* frame in the loop -- not
    /// just the loop head's -- to say `Object`, and only recording the
    /// declaration at the store gets the frames this assembler already emitted
    /// on its single forward pass to agree with the ones the back edge merges.
    pub fn set_local_class(&mut self, slot: u16, name: &str) {
        if name.is_empty() {
            return;
        }
        self.local_class.insert(slot, name.to_string());
    }

    fn set_local(&mut self, n: u16, t: VType) {
        let i = n as usize;
        let t = match (self.local_class.get(&n), &t) {
            // Every reference stored into a declared slot is recorded as the
            // declared class, which by construction is a supertype of it.
            (Some(c), VType::Object(_) | VType::Null) => VType::Object(c.clone()),
            _ => t,
        };
        if i >= self.vlocals.len() {
            self.vlocals.resize(i + 1, VType::Top);
        }
        // An open handler frame has to describe the slot at *every* point of the
        // guarded region, so widen each open snapshot to the common supertype.
        for snap in self.try_locals.iter_mut() {
            if i >= snap.len() {
                snap.resize(i + 1, VType::Top);
            }
            snap[i] = merge_vtype(&snap[i], &t, None);
        }
        self.vlocals[i] = t;
        self.ensure_local(n);
    }

    fn push_v(&mut self, t: VType) {
        let s = t.slots();
        self.vstack.push(t);
        self.bump(s);
    }

    /// The operand stack as the generator sees it, bottom first.
    ///
    /// Entering an exception handler *clears* the operand stack (JVMS 4.10.1.6),
    /// so a `try` generated while values are already pending -- `println(try …)`,
    /// `f(a, try …)`, `new Box(try …)` -- leaves the join after the `try` with
    /// two different stack depths and the frames disagree. The generator parks
    /// those pending values in locals for the duration of the guarded region;
    /// this is what it asks to find out what is there. (scalac solves the same
    /// problem in its `LiftTry` phase, by lifting the `try` into a synthetic
    /// `liftedTree1$1` method called from the argument position.)
    pub fn stack_entries(&self) -> Vec<StackEntry> {
        self.vstack
            .iter()
            .map(|t| match t {
                VType::Integer => StackEntry::Int,
                VType::Long => StackEntry::Long,
                VType::Float => StackEntry::Float,
                VType::Double => StackEntry::Double,
                VType::Object(n) => StackEntry::Ref(Some(n.clone())),
                _ => StackEntry::Ref(None),
            })
            .collect()
    }

    /// Whether the value on top of the stack is a *reference*.
    ///
    /// A branch of a `try` may have boxed its result already (the typer's own
    /// adaptation) or not, and the tree's type does not say which. The slot the
    /// result is parked in has one sort, so the generator asks what is actually
    /// there before deciding to box.
    pub fn top_is_reference(&self) -> bool {
        matches!(
            self.vstack.last(),
            Some(
                VType::Object(_) | VType::Null | VType::UninitializedThis | VType::Uninitialized(_)
            )
        )
    }

    /// The internal name of the object currently on top of the verifier's
    /// model of the stack, if it is one. Used to skip a `checkcast` that the
    /// value already satisfies exactly.
    pub fn top_object(&self) -> Option<&str> {
        match self.vstack.last() {
            Some(VType::Object(n)) => Some(n.as_str()),
            _ => None,
        }
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
        if self.dead {
            // A frame inside a dead region would be dropped with the bytes it
            // describes, and its offset would then point at unrelated code.
            return;
        }
        // Offset 0 is *not* exempt. A `while (true)` whose head is the first
        // instruction of the method is branched back to from the end of the
        // body, and JVMS 4.7.4 requires a frame at every branch target: the
        // first entry's offset is its `offset_delta`, so offset 0 is expressed
        // with a delta of 0. Skipping it produced `VerifyError: Expecting a
        // stackmap frame at branch target 0`.
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
        self.label_join.push(None);
        Label(i)
    }

    /// Tell the merge at `l` what class the value on top of the stack has
    /// there. `name` is an internal class name (or an array descriptor); it has
    /// to be a supertype of everything the branches push, which is exactly what
    /// the expression's static type is.
    pub fn set_join_class(&mut self, l: Label, name: &str) {
        if name.is_empty() || name == "java/lang/Object" {
            return;
        }
        self.label_join[l.0] = Some(name.to_string());
    }

    pub fn mark(&mut self, l: Label) {
        // Whatever was emitted after the last `return`/`athrow`/`goto` can never
        // run. Drop it before the label takes its offset, so the label lands
        // right behind the terminator instead of behind a stretch of code the
        // verifier would still have to make sense of.
        self.drop_dead();
        let off = self.bytes.len() as u16;
        self.labels[l.0] = Some(off);
        if self.dead {
            if let Some(st) = self.label_stack[l.0].clone() {
                self.vstack = st;
                self.stack = self.vstack.iter().map(|t| t.slots()).sum();
                self.revive();
                if let Some(loc) = self.label_locals[l.0].clone() {
                    self.vlocals = loc;
                }
            }
            // Otherwise no jump ever targeted this label and control cannot
            // fall into it either, so the block that follows stays dead and is
            // dropped at the next label (or at `finish`).
        } else {
            if let Some(st) = self.label_stack[l.0].clone() {
                self.vstack = merge_stack(&self.vstack, &st, self.label_join[l.0].as_deref());
                self.stack = self.vstack.iter().map(|t| t.slots()).sum();
            }
            if let Some(loc) = self.label_locals[l.0].clone() {
                self.vlocals = merge_locals(&self.vlocals, &loc);
            }
        }
        if !self.dead && (self.label_stack[l.0].is_some() || self.need_frame) {
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

    /// Snapshot the locals at the start of a guarded region. A handler can be
    /// entered from any point in that region, so its frame may only claim locals
    /// that are already live on entry. Nested `try`s push and pop in order.
    pub fn capture_try_locals(&mut self) {
        self.try_locals.push(self.vlocals.clone());
    }

    /// Pop the snapshot pushed by [`Assembler::capture_try_locals`]; call it once
    /// the region's handlers have been emitted.
    pub fn release_try_locals(&mut self) {
        self.try_locals.pop();
    }

    /// Handler entry: the JVM pushes the exception object. Reset tracked stack.
    pub fn enter_handler(&mut self) {
        self.drop_dead();
        self.revive();
        self.vstack = vec![VType::Object("java/lang/Throwable".into())];
        self.stack = 1;
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
        let off = self.bytes.len() as u16;
        self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
        self.need_frame = false;
    }

    /// Handler entry that restores the locals from [`capture_try_locals`], so the
    /// frame does not claim locals the guarded body initialized later.
    pub fn enter_handler_captured_locals(&mut self) {
        self.drop_dead();
        self.revive();
        self.vstack = vec![VType::Object("java/lang/Throwable".into())];
        self.stack = 1;
        if self.stack > self.max_stack {
            self.max_stack = self.stack;
        }
        if let Some(loc) = self.try_locals.last().cloned() {
            self.vlocals = loc;
        }
        let off = self.bytes.len() as u16;
        self.record_frame_at(off, self.vstack.clone(), self.vlocals.clone());
        self.need_frame = false;
    }

    fn emit_op(&mut self, op: u8) {
        if self.need_frame && !self.dead {
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
        if op == 0xa7 && self.dead {
            // `if (c) throw e else v`: the branch's trailing `goto end` sits
            // after the `athrow`, so it can never run. Emitting it anyway made
            // `end` inherit the dead branch's empty stack and the verifier then
            // rejected the live branch's value.
            return;
        }
        let n_pop = (-stack_delta).max(0) as usize;
        self.pop_n_v(n_pop);
        self.emit_op(op);
        self.patches.push((self.bytes.len(), l));
        self.emit_u16(0);
        if op != 0xa7 && !self.dead {
            // The fall-through of a conditional branch becomes a branch target
            // if the branch has to be widened; see `cond_frames`. Recording it
            // costs one snapshot per conditional branch -- the same order as
            // `save_label` just below, which every branch already pays.
            let off = self.bytes.len() as u16;
            self.cond_frames
                .push((off, self.vstack.clone(), self.vlocals.clone()));
        }
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
            // Everything up to the next reachable label is dead. Drop the
            // operand stack the way a return does: an irrefutable last case in
            // a `PartialFunction` leaves the `applyOrElse` default branch
            // unreachable, and a frame that still claimed the case body's
            // value made the verifier reject the fall-through `areturn`.
            self.kill();
        }
        let _ = stack_delta;
    }

    fn save_label(&mut self, l: Label) {
        if self.dead {
            // A jump we are about to drop must not contribute its (empty) state
            // to the target's merged frame.
            return;
        }
        let stack = self.vstack.clone();
        let locals = self.vlocals.clone();
        self.label_stack[l.0] = Some(match self.label_stack[l.0].take() {
            Some(old) => merge_stack(&old, &stack, self.label_join[l.0].as_deref()),
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
    fn bin_float(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Float);
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
    pub fn lrem(&mut self) {
        self.bin_long(0x71);
    }
    /// A `long` shift takes an `int` count, so the stack loses only the count.
    pub fn lshl(&mut self) {
        self.shift_long(0x79);
    }
    pub fn lshr(&mut self) {
        self.shift_long(0x7b);
    }
    pub fn lushr(&mut self) {
        self.shift_long(0x7d);
    }
    fn shift_long(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Long);
    }
    pub fn land(&mut self) {
        self.bin_long(0x7f);
    }
    pub fn lor(&mut self) {
        self.bin_long(0x81);
    }
    pub fn lxor(&mut self) {
        self.bin_long(0x83);
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
    pub fn drem(&mut self) {
        self.bin_double(0x73);
    }
    pub fn dneg(&mut self) {
        self.emit_op(0x77);
    }

    pub fn fadd(&mut self) {
        self.bin_float(0x62);
    }
    pub fn fsub(&mut self) {
        self.bin_float(0x66);
    }
    pub fn fmul(&mut self) {
        self.bin_float(0x6a);
    }
    pub fn fdiv(&mut self) {
        self.bin_float(0x6e);
    }
    pub fn frem(&mut self) {
        self.bin_float(0x72);
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
    pub fn l2i(&mut self) {
        self.emit_op(0x88);
        let _ = self.pop_v();
        self.push_v(VType::Integer);
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
    /// `fcmpg` / `dcmpg`: like `fcmpl` / `dcmpl` but NaN pushes 1, which is
    /// what `<` and `<=` need so that a NaN operand compares false.
    pub fn fcmpg(&mut self) {
        self.emit_op(0x96);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn dcmpg(&mut self) {
        self.emit_op(0x98);
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
    pub fn f2i(&mut self) {
        self.emit_op(0x8b);
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn f2l(&mut self) {
        self.emit_op(0x8c);
        let _ = self.pop_v();
        self.push_v(VType::Long);
    }
    pub fn d2i(&mut self) {
        self.emit_op(0x8e);
        let _ = self.pop_v();
        self.push_v(VType::Integer);
    }
    pub fn d2l(&mut self) {
        self.emit_op(0x8f);
        let _ = self.pop_v();
        self.push_v(VType::Long);
    }
    pub fn d2f(&mut self) {
        self.emit_op(0x90);
        let _ = self.pop_v();
        self.push_v(VType::Float);
    }
    /// `i2b` / `i2c` / `i2s` leave an `int` on the stack, so the verification
    /// type is unchanged and there is nothing to pop or push.
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
        if !self.dead {
            self.dead_start = Some(self.bytes.len());
        }
        self.dead = true;
        self.need_frame = true;
    }

    /// Discard the instructions emitted since the last terminator. Codegen keeps
    /// emitting through unreachable positions (`throw` in expression position
    /// still has to be followed by the method's `ireturn`); rather than teaching
    /// every emitter about reachability, we let it emit and rewind here.
    fn drop_dead(&mut self) {
        let Some(start) = self.dead_start else { return };
        if self.bytes.len() <= start {
            return;
        }
        self.bytes.truncate(start);
        self.patches.retain(|(at, _)| *at < start);
        self.patches_i32.retain(|(at, _, _)| *at < start);
        let keep = start as u16;
        self.frames.retain(|&off, _| off <= keep);
        // `cond_frames` is in emission order, so the dropped tail is a suffix.
        // A `retain` here would be quadratic: `drop_dead` runs at every label.
        while self.cond_frames.last().is_some_and(|&(off, ..)| off > keep) {
            self.cond_frames.pop();
        }
    }

    /// Reachable code starts here again.
    fn revive(&mut self) {
        self.dead = false;
        self.dead_start = None;
    }

    pub fn getstatic(&mut self, owner: &str, name: &str, desc: &str) {
        // Field names are encoded like method names (`/` is `$div`); see
        // `ClassEmit::write_with_pool`.
        let i = self.pool.fieldref(owner, &encode_method_name(name), desc);
        self.emit_op(0xb2);
        self.emit_u16(i);
        self.push_v(vtype_from_desc(desc));
    }
    pub fn putstatic(&mut self, owner: &str, name: &str, desc: &str) {
        // Field names are encoded like method names (`/` is `$div`); see
        // `ClassEmit::write_with_pool`.
        let i = self.pool.fieldref(owner, &encode_method_name(name), desc);
        self.emit_op(0xb3);
        self.emit_u16(i);
        let _ = self.pop_v();
    }
    pub fn getfield(&mut self, owner: &str, name: &str, desc: &str) {
        // Field names are encoded like method names (`/` is `$div`); see
        // `ClassEmit::write_with_pool`.
        let i = self.pool.fieldref(owner, &encode_method_name(name), desc);
        self.emit_op(0xb4);
        self.emit_u16(i);
        let _ = self.pop_v();
        self.push_v(vtype_from_desc(desc));
    }
    pub fn putfield(&mut self, owner: &str, name: &str, desc: &str) {
        // Field names are encoded like method names (`/` is `$div`); see
        // `ClassEmit::write_with_pool`.
        let i = self.pool.fieldref(owner, &encode_method_name(name), desc);
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
        // JVMS 6.5 `invokeinterface`: `count` is the number of argument
        // *slots* plus one for the receiver, so a `long` or `double`
        // parameter counts twice. Counting parameters instead made
        // `reificationSupport.FlagsRepr(8192L)` -- an interface method taking
        // one `Long` -- fail to verify with "Inconsistent args count operand
        // in invokeinterface".
        let n_args = 1 + count_param_slots(desc);
        self.bytes.push(n_args as u8);
        self.bytes.push(0);
        self.apply_invoke(desc, true, false, owner);
    }

    /// `invokedynamic` (JVMS §6.5) against
    /// `java.lang.invoke.LambdaMetafactory.metafactory`: the call site takes
    /// the captured values and returns an instance of the functional
    /// interface `call_desc` names as its return type.
    ///
    /// * `sam_name` / `sam_desc` — the interface's single abstract method.
    /// * `call_desc` — `(<captured types>)L<functional interface>;`.
    /// * `impl_*` — the static method holding the lambda body. It is always a
    ///   method of a *class*, never of an interface, because JVMS §4.6 forbids
    ///   the flags the body carries on an interface method.
    ///
    /// `samMethodType` and `instantiatedMethodType` are both `sam_desc`: the
    /// body is written at the erased `(Object…)Object` shape, so
    /// `LambdaMetafactory` has nothing to adapt and never needs a bridge.
    pub fn invokedynamic_lambda(
        &mut self,
        sam_name: &str,
        sam_desc: &str,
        call_desc: &str,
        impl_owner: &str,
        impl_name: &str,
        impl_desc: &str,
    ) {
        const MF_OWNER: &str = "java/lang/invoke/LambdaMetafactory";
        const MF_DESC: &str = "(Ljava/lang/invoke/MethodHandles$Lookup;\
Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodType;\
Ljava/lang/invoke/MethodHandle;Ljava/lang/invoke/MethodType;)\
Ljava/lang/invoke/CallSite;";
        let bsm = self
            .pool
            .method_handle_static(MF_OWNER, "metafactory", MF_DESC, false);
        let a0 = self.pool.method_type(sam_desc);
        let a1 = self
            .pool
            .method_handle_static(impl_owner, impl_name, impl_desc, false);
        let bsm_index = self.pool.bootstrap(bsm, vec![a0, a1, a0]);
        let i = self.pool.invoke_dynamic(bsm_index, sam_name, call_desc);
        self.emit_op(0xba);
        self.emit_u16(i);
        self.bytes.push(0);
        self.bytes.push(0);
        self.apply_invoke(call_desc, false, false, MF_OWNER);
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
                // JVMS 4.10.1.9: `uninitializedThis` becomes the class *being
                // verified*, not the one whose `<init>` was invoked. Those
                // differ for the super constructor call every subclass makes,
                // and writing the superclass made every frame after it claim
                // `this` was a `B`: `class C(n: Int) extends B("b") { val e =
                // if (n > 0) … else … }` was a `VerifyError: Bad type on
                // operand stack in putfield`, `'B' … is not assignable to 'C'`.
                let inited = VType::Object(self.this_name.clone());
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

    /// `laload` / `faload` / `daload` / `baload` / `caload` / `saload`: pop the
    /// index and the array, push the element. `baload` serves both `[B` and
    /// `[Z`, and `baload` / `caload` / `saload` all push an `int`.
    fn typed_aload(&mut self, op: u8, elem: VType) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        self.push_v(elem);
    }
    pub fn laload(&mut self) {
        self.typed_aload(0x2f, VType::Long);
    }
    pub fn faload(&mut self) {
        self.typed_aload(0x30, VType::Float);
    }
    pub fn daload(&mut self) {
        self.typed_aload(0x31, VType::Double);
    }
    pub fn baload(&mut self) {
        self.typed_aload(0x33, VType::Integer);
    }
    pub fn caload(&mut self) {
        self.typed_aload(0x34, VType::Integer);
    }
    pub fn saload(&mut self) {
        self.typed_aload(0x35, VType::Integer);
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

    /// `lastore` / `fastore` / `dastore` / `bastore` / `castore` / `sastore`:
    /// pop the value, the index and the array.
    fn typed_astore(&mut self, op: u8) {
        self.emit_op(op);
        let _ = self.pop_v();
        let _ = self.pop_v();
        let _ = self.pop_v();
    }
    pub fn lastore(&mut self) {
        self.typed_astore(0x50);
    }
    pub fn fastore(&mut self) {
        self.typed_astore(0x51);
    }
    pub fn dastore(&mut self) {
        self.typed_astore(0x52);
    }
    pub fn bastore(&mut self) {
        self.typed_astore(0x54);
    }
    pub fn castore(&mut self) {
        self.typed_astore(0x55);
    }
    pub fn sastore(&mut self) {
        self.typed_astore(0x56);
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

    /// Rewrite the branches whose 16-bit offset does not reach their label.
    ///
    /// `goto` has a wide form (`goto_w`, JVMS 6.5) but no conditional branch
    /// does, so a conditional that cannot reach becomes the inverse branch
    /// over a `goto_w` -- which is what nsc emits too, by way of ASM:
    ///
    /// ```text
    ///        ifeq L                ifne SKIP
    ///                     ==>      goto_w L
    ///                       SKIP:
    /// ```
    ///
    /// Growing the code moves every offset behind the rewrite, which can put
    /// a further branch out of reach, so the choice runs to a fixpoint before
    /// a byte is moved. Each rewrite grows by a multiple of four (padded with
    /// `nop`s that no path reaches) so that the alignment padding of a
    /// `tableswitch`/`lookupswitch` never has to change size with it.
    ///
    /// A method over 64 KB is left alone: its label offsets have already
    /// wrapped, and `add_code` rejects it rather than writing it.
    fn widen_jumps(&mut self) {
        if self.bytes.len() > crate::classfile::MAX_CODE_LENGTH {
            return;
        }
        // The fast path, and the only cost every other method pays: if all the
        // offsets reach, there is nothing to do.
        let mut any = false;
        for &(at, lab) in &self.patches {
            let target = self.labels[lab.0].unwrap_or(0) as i64;
            if !(MIN_BRANCH..=MAX_BRANCH).contains(&(target - (at as i64 - 1))) {
                any = true;
                break;
            }
        }
        if !any {
            return;
        }

        // (opcode position, label, opcode, widened?), by position.
        let mut sites: Vec<(usize, usize, u8, bool)> = self
            .patches
            .iter()
            .map(|&(at, lab)| (at - 1, lab.0, self.bytes[at - 1], false))
            .collect();
        sites.sort_by_key(|s| s.0);

        // `grown[k]` is how many bytes the first `k` sites add. A position
        // maps to itself plus what every site *before* it adds, so a label on
        // a widened branch still lands on that branch's first byte.
        let mut grown: Vec<usize> = vec![0; sites.len() + 1];
        let recompute = |sites: &[(usize, usize, u8, bool)], grown: &mut Vec<usize>| {
            for (k, s) in sites.iter().enumerate() {
                grown[k + 1] = grown[k] + if s.3 { widened_growth(s.2) } else { 0 };
            }
        };
        let map = |sites: &[(usize, usize, u8, bool)], grown: &[usize], pc: usize| -> usize {
            pc + grown[sites.partition_point(|s| s.0 < pc)]
        };
        loop {
            recompute(&sites, &mut grown);
            let mut changed = false;
            for i in 0..sites.len() {
                if sites[i].3 {
                    continue;
                }
                let target = self.labels[sites[i].1].unwrap_or(0) as usize;
                let rel =
                    map(&sites, &grown, target) as i64 - map(&sites, &grown, sites[i].0) as i64;
                if !(MIN_BRANCH..=MAX_BRANCH).contains(&rel) {
                    sites[i].3 = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        recompute(&sites, &mut grown);

        let mut out: Vec<u8> = Vec::with_capacity(self.bytes.len() + grown[sites.len()]);
        let mut copied = 0usize;
        for &(op_pc, lab, op, wide) in &sites {
            out.extend_from_slice(&self.bytes[copied..op_pc]);
            copied = op_pc + 3;
            let here = out.len();
            debug_assert_eq!(here, map(&sites, &grown, op_pc));
            let target = map(&sites, &grown, self.labels[lab].unwrap_or(0) as usize) as i64;
            if !wide {
                out.push(op);
                out.extend_from_slice(&((target - here as i64) as i16).to_be_bytes());
            } else if op == 0xa7 {
                // The `nop`s go *first*. JVMS 4.10.1 wants a frame on the
                // instruction after an unconditional branch, and padding
                // behind the `goto_w` would put unreachable `nop`s there
                // ("Expecting a stack map frame ... @23: nop").
                out.extend_from_slice(&[0x00, 0x00]); // growth stays 4
                out.push(0xc8); // goto_w
                out.extend_from_slice(&((target - (here as i64 + 2)) as i32).to_be_bytes());
            } else {
                // The inverse branch skips the `goto_w`, so its target is the
                // fall-through -- `map(op_pc + 3)`, eleven bytes past `here`.
                out.extend_from_slice(&[0x00, 0x00, 0x00]); // growth stays 8
                out.push(invert_branch(op));
                out.extend_from_slice(&8i16.to_be_bytes());
                out.push(0xc8); // goto_w
                out.extend_from_slice(&((target - (here as i64 + 6)) as i32).to_be_bytes());
            }
        }
        out.extend_from_slice(&self.bytes[copied..]);
        debug_assert_eq!(out.len(), self.bytes.len() + grown[sites.len()]);

        // Everything that names a code offset moves with the code.
        for l in self.labels.iter_mut().flatten() {
            *l = map(&sites, &grown, *l as usize) as u16;
        }
        for (at, _, op_pc) in self.patches_i32.iter_mut() {
            *at = map(&sites, &grown, *at);
            *op_pc = map(&sites, &grown, *op_pc);
        }
        let old_frames = std::mem::take(&mut self.frames);
        for (off, (mut locals, mut stack)) in old_frames {
            for t in locals.iter_mut().chain(stack.iter_mut()) {
                if let VType::Uninitialized(at) = t {
                    *t = VType::Uninitialized(map(&sites, &grown, *at as usize) as u16);
                }
            }
            self.frames
                .insert(map(&sites, &grown, off as usize) as u16, (locals, stack));
        }
        // A widened conditional's fall-through is now a branch target. When a
        // label already sits there the frame recorded for it is the merge of
        // *every* path that arrives, so it wins over this one path's state.
        for &(op_pc, _, op, wide) in &sites {
            if !wide || op == 0xa7 {
                continue;
            }
            let after = (op_pc + 3) as u16;
            let Ok(k) = self.cond_frames.binary_search_by_key(&after, |&(o, ..)| o) else {
                debug_assert!(false, "no fall-through frame for the branch at {op_pc}");
                continue;
            };
            let (_, stack, locals) = self.cond_frames[k].clone();
            self.frames
                .entry(map(&sites, &grown, after as usize) as u16)
                .or_insert((locals, stack));
        }

        self.bytes = out;
        // Every 16-bit offset is written above; leave nothing for `finish` to
        // patch on top of the rewritten code.
        self.patches.clear();
    }

    pub fn finish(mut self) -> (Code, Pool) {
        // Trailing unreachable code would leave the method without a terminator
        // in the eyes of the verifier ("falls through code end"), so drop it and
        // let the `return`/`athrow` that killed it end the method.
        self.drop_dead();
        // `end` is a byte count, not an offset: `as u16` used to wrap it for a
        // method over 64 KB and the `retain` below then threw away nearly
        // every frame. Such a method is not encodable at all -- `add_code`
        // reports it -- but it must not be *silently* mangled on the way out.
        let end = self.bytes.len();
        self.frames.retain(|&off, _| (off as usize) < end);
        // Branches whose 16-bit offset does not reach their label are rewritten
        // here, which moves everything after them; the pass patches every
        // 16-bit branch itself and clears `patches` when it does anything.
        self.widen_jumps();
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
            if start_pc >= end_pc {
                // The whole guarded region was unreachable and got dropped. An
                // empty range is rejected by the verifier and protects nothing.
                continue;
            }
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

/// How many bytes the wide form of `op` adds to the three a short branch takes.
///
/// `goto` needs 5 (`goto_w`) and a conditional needs 8 (itself, inverted, plus
/// a `goto_w`); both are rounded up to a multiple of four with `nop`s so that
/// a `tableswitch`/`lookupswitch` behind them keeps its alignment padding.
fn widened_growth(op: u8) -> usize {
    if op == 0xa7 {
        4
    } else {
        8
    }
}

/// The branch that jumps exactly when `op` does not (JVMS 6.5). The pairs are
/// adjacent: `ifeq`/`ifne`, `iflt`/`ifge`, `ifgt`/`ifle`, the six `if_icmp*`,
/// the two `if_acmp*`, and `ifnull`/`ifnonnull`.
fn invert_branch(op: u8) -> u8 {
    match op {
        0x99..=0xa6 => {
            if (op - 0x99).is_multiple_of(2) {
                op + 1
            } else {
                op - 1
            }
        }
        0xc6 => 0xc7,
        0xc7 => 0xc6,
        _ => unreachable!("{op:#x} is not a conditional branch"),
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
            // `strip_prefix`, not `trim_start_matches`: the latter eats *every*
            // leading `L`, so a class in the default package whose name starts
            // with one (`LK`, `Lib$`) lost its own first letter and the frame
            // named a class that does not exist
            // (`NoClassDefFoundError: K`).
            let inner = desc
                .strip_prefix('L')
                .and_then(|s| s.strip_suffix(';'))
                .unwrap_or(desc);
            VType::Object(inner.to_string())
        }
        _ => VType::Object("java/lang/Object".into()),
    }
}

pub fn param_descs(desc: &str) -> Vec<String> {
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

/// `top_lub` applies to the topmost slot only: that is the value the join is
/// *about*. The slots under it were pushed before the branch and are the same
/// on every path, so they never actually differ.
///
/// Everything else merges to `java/lang/Object`, which is always a valid frame
/// type. This used to be the *method's return type* -- a guess that made an
/// `areturn` after a `match` verify, and made everything else fail: a `String`
/// and an `Integer` merged inside a method returning `Option` were declared
/// `scala/Option`, and `Some(n match { case 1 => "one"; case _ => n })` was
/// `VerifyError: Inconsistent stackmap frames`. `set_join_class` states the
/// real type where one is known.
fn merge_stack(a: &[VType], b: &[VType], top_lub: Option<&str>) -> Vec<VType> {
    if a.len() != b.len() {
        if a.len() < b.len() {
            return a.to_vec();
        }
        return b.to_vec();
    }
    let top = a.len().saturating_sub(1);
    a.iter()
        .zip(b.iter())
        .enumerate()
        .map(|(i, (x, y))| merge_vtype(x, y, if i == top { top_lub } else { None }))
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

/// The number of local-variable slots a descriptor's parameters occupy:
/// `long` and `double` take two, everything else one. Only `invokeinterface`'s
/// `count` operand needs this; the verifier's stack model has one entry per
/// value.
fn count_param_slots(desc: &str) -> usize {
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut n = 0;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        n += if c == 'J' || c == 'D' { 2 } else { 1 };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn asm() -> Assembler {
        let mut a = Assembler::new(1);
        a.init_method(ACC_STATIC, "m", "()V", "T");
        a
    }

    /// The number of entries in the `StackMapTable` body (JVMS 4.7.4).
    fn frame_count(code: &Code) -> u16 {
        let b = code.stack_map.as_ref().expect("a stack map");
        u16::from_be_bytes([b[0], b[1]])
    }

    #[test]
    fn a_branch_that_reaches_is_left_alone() {
        let mut a = asm();
        let l = a.fresh_label();
        a.iconst(0);
        a.ifeq(l);
        for _ in 0..100 {
            a.nop();
        }
        a.mark(l);
        a.vreturn();
        let (code, _) = a.finish();
        assert_eq!(code.bytes.len(), 1 + 3 + 100 + 1);
        assert_eq!(code.bytes[1], 0x99, "still a plain `ifeq`");
        assert_eq!(&code.bytes[2..4], &[0x00, 103]);
        assert_eq!(frame_count(&code), 1, "only the branch target needs one");
    }

    #[test]
    fn a_conditional_that_cannot_reach_is_inverted_over_goto_w() {
        let mut a = asm();
        let l = a.fresh_label();
        a.iconst(0);
        a.ifeq(l);
        for _ in 0..40_000 {
            a.nop();
        }
        a.mark(l);
        a.vreturn();
        let (code, _) = a.finish();
        assert_eq!(code.bytes.len(), 1 + 11 + 40_000 + 1);
        assert_eq!(&code.bytes[1..4], &[0x00, 0x00, 0x00], "alignment padding");
        assert_eq!(code.bytes[4], 0x9a, "`ifne`, the inverse of `ifeq`");
        assert_eq!(&code.bytes[5..7], &[0x00, 8], "over the `goto_w`");
        assert_eq!(code.bytes[7], 0xc8, "`goto_w`");
        let target: i32 = 1 + 11 + 40_000;
        assert_eq!(&code.bytes[8..12], &(target - 7).to_be_bytes());
        // The fall-through of the inverse branch is a branch target now, so it
        // takes a frame of its own on top of the one the label already had.
        assert_eq!(frame_count(&code), 2);
    }

    #[test]
    fn a_goto_that_cannot_reach_becomes_goto_w() {
        let mut a = asm();
        let top = a.fresh_label();
        a.mark(top);
        for _ in 0..40_000 {
            a.nop();
        }
        a.goto(top);
        let (code, _) = a.finish();
        assert_eq!(code.bytes.len(), 40_000 + 7);
        assert_eq!(&code.bytes[40_000..40_002], &[0x00, 0x00], "padding first");
        assert_eq!(code.bytes[40_002], 0xc8);
        assert_eq!(&code.bytes[40_003..40_007], &(-40_002i32).to_be_bytes());
        assert_eq!(frame_count(&code), 1);
    }

    /// Widening one branch moves the code behind it, which can put a branch
    /// that *did* reach out of range. The choice has to run to a fixpoint.
    #[test]
    fn widening_cascades() {
        // `back` is at 0 and `top` at 1000. The inner branch at 33_000 reaches
        // back past -32768 and is widened; that pushes the outer branch at
        // 33_768 -- whose offset to `top` is exactly -32768, the last one that
        // still fits -- eight bytes further away.
        let mut a = asm();
        let back = a.fresh_label();
        let top = a.fresh_label();
        a.mark(back);
        for _ in 0..1000 {
            a.nop();
        }
        a.mark(top);
        for _ in 0..31_999 {
            a.nop();
        }
        a.iconst(0); // 32_999
        a.ifeq(back); // 33_000, offset -33_000: does not reach
        for _ in 0..764 {
            a.nop();
        }
        a.iconst(0); // 33_767
        a.ifne(top); // 33_768, offset -32_768: reaches, until the above widens
        a.vreturn();
        let (code, _) = a.finish();
        assert_eq!(code.bytes[33_003], 0x9a, "the inner `ifeq`, inverted");
        assert_eq!(code.bytes[33_006], 0xc8);
        assert_eq!(code.bytes[33_779], 0x99, "the outer `ifne`, inverted");
        assert_eq!(code.bytes[33_782], 0xc8);
        // 33_772 bytes before the rewrite, plus eight for each of the two.
        assert_eq!(code.bytes.len(), 33_772 + 8 + 8);
    }
}
