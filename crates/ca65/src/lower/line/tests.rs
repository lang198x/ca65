//! Whole programs, run through the expander and lowered line by line, then
//! assembled by a two-pass assembler as small as the tests need, against
//! the bytes ca65 V2.19 and ld65 wrote under a flat link at 0.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use asm198x_ir::{DiagSeverity, Expr as Ir, Operation, align_pad, next_pc};
use isa198x::OperandKind;
use lang198x_syntax::DiagnosticKind;

use super::line;
use crate::Profile;
use crate::ast::{Expr, NameRef};
use crate::expand::{Expander, Finished, Host};
use crate::lower::{
    self, Binary, Declared, Known, Scope, Segment, Size, Value, Visibility, Visible,
};

/// The assembler: what each name is known as so far, and the final values
/// from the pass before.
#[derive(Default)]
struct Mini {
    known: HashMap<String, Value>,
    finals: HashMap<String, i64>,
    /// The bytes each label's line made.
    sizes: HashMap<String, i64>,
    pc: i64,
    org: bool,
    /// The names read so far.
    read: RefCell<HashSet<String>>,
    /// The names the line's token functions read, each with how many
    /// functions had finished, kept aside until the line is lowered.
    function_reads: RefCell<Vec<(usize, String)>>,
    /// While a token function's value is lowered, how many had finished.
    in_function: Cell<Option<usize>>,
    /// The bank every label is in, for `.bank`, as a linker would give it,
    /// or `None` to leave `.bank` not lowered.
    bank: Option<i64>,
    /// Each value `.bank` was asked about.
    banked: RefCell<Vec<Ir>>,
    /// The character map's changes, and the maps kept by `.pushcharmap`.
    charmap: HashMap<u8, u8>,
    kept: Vec<HashMap<u8, u8>>,
}

impl Mini {
    /// Counts the names the line's token functions read as read.
    fn record_function_reads(&self) {
        let read = self.function_reads.take();
        self.read
            .borrow_mut()
            .extend(read.into_iter().map(|(_, name)| name));
    }

    fn was_read_before(&self, name: &str, finished: usize) -> bool {
        self.read.borrow().contains(name)
            || self
                .function_reads
                .borrow()
                .iter()
                .any(|(at, read)| *at < finished && read == name)
    }

    fn value(&self, expr: &Ir) -> Option<i64> {
        let lookup = |name: &str| match self.known.get(name) {
            _ if name == BANK => self.bank,
            Some(Value::Constant(value)) => Some(*value),
            _ => self.finals.get(name).copied(),
        };
        expr.eval_with(&lookup, Some(self.pc), 0).ok()
    }
}

impl Known for Mini {
    fn name(&self, name: &str) -> Value {
        self.known.get(name).copied().unwrap_or(Value::Unknown)
    }

    fn pc(&self) -> Value {
        if self.org {
            Value::Constant(self.pc)
        } else {
            Value::Address(Size::Absolute)
        }
    }

    fn size_of(&self, name: &str) -> Option<i64> {
        self.sizes.get(name).copied()
    }

    fn is_defined(&self, name: &str) -> bool {
        self.known.contains_key(name)
    }

    fn is_referenced(&self, name: &str) -> bool {
        self.was_read_before(name, usize::MAX)
    }

    fn is_referenced_before(&self, name: &str, finished: usize) -> bool {
        self.was_read_before(name, finished)
    }

    fn read(&self, name: &str) {
        match self.in_function.get() {
            Some(finished) => self
                .function_reads
                .borrow_mut()
                .push((finished, name.to_owned())),
            None => {
                self.read.borrow_mut().insert(name.to_owned());
            }
        }
    }

    fn translate(&self, character: u8) -> u8 {
        self.charmap.get(&character).copied().unwrap_or(character)
    }

    /// A name for the linker to fill in, which is never a constant here.
    fn bank(&self, value: &Ir) -> Option<Ir> {
        self.banked.borrow_mut().push(value.clone());
        self.bank.map(|_| Ir::Sym(BANK.to_owned()))
    }
}

/// The name `.bank` stands for until the link.
const BANK: &str = "\u{1}bank";

impl Host for Mini {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        let lowered = lower::expression(expr, &*self).map_err(|d| d.message().to_owned())?;
        self.value(&lowered).ok_or_else(|| "not known".to_owned())
    }

    fn is_defined(&mut self, name: &NameRef) -> bool {
        self.known.contains_key(&name.text())
    }

    fn evaluate_in_function(&mut self, expr: &Expr, finished: usize) -> Result<i64, String> {
        self.in_function.set(Some(finished));
        let value = self.evaluate(expr);
        self.in_function.set(None);
        value
    }

    fn evaluate_condition(&mut self, expr: &Expr, finished: &Finished) -> Result<i64, String> {
        let lowered = lower::condition(expr, &*self, finished);
        self.record_function_reads();
        let lowered = lowered.map_err(|d| d.message().to_owned())?;
        self.value(&lowered).ok_or_else(|| "not known".to_owned())
    }

    fn is_referenced(&mut self, name: &NameRef) -> bool {
        self.was_read_before(&name.text(), usize::MAX)
    }

    fn is_constant(&mut self, expr: &Expr) -> bool {
        self.evaluate(expr).is_ok()
    }

    fn translate(&mut self, character: u8) -> u8 {
        Known::translate(self, character)
    }
}

/// One pass over `source`, given the final values from the pass before:
/// the bytes, and every name's value.
fn pass(source: &str, finals: HashMap<String, i64>) -> Result<(Vec<u8>, Mini), String> {
    pass_banked(source, finals, None)
}

/// One pass, with every label in `bank` for `.bank`.
fn pass_banked(
    source: &str,
    finals: HashMap<String, i64>,
    bank: Option<i64>,
) -> Result<(Vec<u8>, Mini), String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).map_err(|e| e.to_string())?;
    let mut mini = Mini {
        finals,
        bank,
        ..Mini::default()
    };
    let mut bytes = Vec::new();
    while let Some(active) = expander.next_line(&mut mini) {
        let lowered = line(&active, profile, &mini).map_err(|d| d.message().to_owned())?;
        mini.record_function_reads();
        match lowered.charmap {
            Some(lower::Charmap::Set { index, value }) => {
                mini.charmap.insert(index, value);
            }
            Some(lower::Charmap::Push) => mini.kept.push(mini.charmap.clone()),
            Some(lower::Charmap::Pop) => {
                mini.charmap = mini.kept.pop().ok_or("Charmap stack is empty")?;
            }
            _ => {}
        }
        let before = bytes.len();
        let mut labels = Vec::new();
        for item in lowered.items {
            if let Some(label) = item.label {
                let value = match &item.op {
                    Some(Operation::Equ(e) | Operation::Set(e)) => match mini.value(e) {
                        Some(v) => Value::Constant(v),
                        None => Value::Unknown,
                    },
                    _ if mini.org => Value::Constant(mini.pc),
                    _ => Value::Address(Size::Absolute),
                };
                let fin = match (&item.op, value) {
                    (_, Value::Constant(v)) => v,
                    (Some(Operation::Equ(e) | Operation::Set(e)), _) => mini.value(e).unwrap_or(0),
                    _ => mini.pc,
                };
                mini.known.insert(label.clone(), value);
                if !matches!(item.op, Some(Operation::Equ(_) | Operation::Set(_))) {
                    labels.push(label.clone());
                }
                mini.finals.entry(label).or_insert(fin);
            }
            let Some(op) = item.op else { continue };
            emit(&op, &mut mini, &mut bytes)?;
        }
        let made = i64::try_from(bytes.len() - before).unwrap_or(i64::MAX);
        for label in labels {
            mini.sizes.insert(label, made);
        }
    }
    if let Some((_, problem)) = expander.diagnostics().first() {
        return Err(problem.message().to_owned());
    }
    Ok((bytes, mini))
}

fn emit(op: &Operation, mini: &mut Mini, bytes: &mut Vec<u8>) -> Result<(), String> {
    let out_of_range = || "out of range".to_owned();
    match op {
        Operation::Equ(_) | Operation::Set(_) => return Ok(()),
        Operation::Org(e) => {
            mini.pc = mini.value(e).ok_or_else(out_of_range)?;
            mini.org = true;
            return Ok(());
        }
        Operation::Sized {
            width,
            big_endian,
            values,
            ..
        } => {
            for value in values {
                let v = mini.value(value).unwrap_or(0);
                if v < 0 || v >= 1_i64 << (8 * width) {
                    return Err(out_of_range());
                }
                let mut le: Vec<u8> = (0..*width).map(|i| (v >> (8 * i)) as u8).collect();
                if *big_endian {
                    le.reverse();
                }
                bytes.extend(le);
            }
        }
        Operation::PseudoPc(None) => {
            // Flat at 0, the segment's address is where its bytes are.
            mini.pc = i64::try_from(bytes.len()).map_err(|_| out_of_range())?;
            mini.org = false;
            return Ok(());
        }
        Operation::Reserve(n) => bytes.extend(std::iter::repeat_n(0, *n)),
        Operation::Fill { count, value } => {
            let n = usize::try_from(mini.value(count).unwrap_or(0)).map_err(|_| out_of_range())?;
            bytes.extend(std::iter::repeat_n(*value, n));
        }
        Operation::AlignTo { modulus, fill } => {
            let n = usize::try_from(align_pad(mini.pc, *modulus)).map_err(|_| out_of_range())?;
            bytes.extend(std::iter::repeat_n(fill.first().copied().unwrap_or(0), n));
        }
        Operation::Instruction {
            mnemonic,
            mode,
            operands,
        } => {
            let form = isa198x::mos6502::SET
                .find_form(mnemonic, mode)
                .ok_or_else(out_of_range)?;
            let len = 1 + form
                .operands
                .iter()
                .map(|o| i64::from(o.bytes))
                .sum::<i64>();
            bytes.extend(form.opcode);
            for (slot, value) in form.operands.iter().zip(operands) {
                let v = mini.value(value).unwrap_or(0);
                let v = if slot.kind == OperandKind::RelativePc {
                    v - mini.pc - len
                } else {
                    v
                };
                bytes.extend((0..slot.bytes).map(|i| (v >> (8 * i)) as u8));
            }
        }
        Operation::Diagnose {
            severity: DiagSeverity::Error,
            message,
        } => return Err(message.clone()),
        Operation::Diagnose { .. } => return Ok(()),
        Operation::Assert {
            cond,
            fatal,
            message,
        } => {
            return match mini.value(cond) {
                Some(0) if *fatal => Err(message.clone()),
                _ => Ok(()),
            };
        }
        _ => return Err("not in this test's assembler".to_owned()),
    }
    mini.pc = next_pc(op, mini.pc, &isa198x::mos6502::SET, None, 1, 0).map_err(|e| e.message)?;
    Ok(())
}

/// `source`, assembled in two passes, as hex.
fn assemble(source: &str) -> Result<String, String> {
    let (_, first) = pass(source, HashMap::new())?;
    let (bytes, _) = pass(source, first.finals)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Programs measured on ca65 V2.19, with the bytes it wrote, or `None` if it
/// refused the program.
const MEASURED: &[(&str, Option<&str>)] = &[
    (".byte 1, $ff, \"ab\", 'c'\n", Some("01ff616263")),
    (".byt 1\n", Some("01")),
    (".byte 256\n", None),
    (".byte -1\n", None),
    (".byte -129\n", None),
    (".byte\n", None),
    (".word $1234, 1\n.addr 2\n", Some("341201000200")),
    (".word -1\n", None),
    (".word $10000\n", None),
    (".dbyt $1234\n", Some("1234")),
    (
        ".faraddr $123456\n.dword $12345678\n",
        Some("56341278563412"),
    ),
    (
        ".lobytes $1234, $5678\n.hibytes $1234\n.bankbytes $123456\n",
        Some("34781212"),
    ),
    (".asciiz \"ab\"\n", Some("616200")),
    (".asciiz \"a\", \"b\"\n", Some("616200")),
    (".asciiz \"a\",1\n", None),
    (".asciiz 'a'\n", None),
    (".asciiz\n", None),
    (".res 3\n.res 2, $ea\n.byte 1\n", Some("000000eaea01")),
    (".res 0\n", Some("")),
    (".res -1\n", None),
    (".res fwd\nfwd=2\n", None),
    (".res 2, 256\n", None),
    (".org $10\nl: .byte <l\n", Some("10")),
    ("nop\n.align 4\n.byte 1\n", Some("ea00000001")),
    ("nop\n.align 4, $ea\n.byte 1\n", Some("eaeaeaea01")),
    ("nop\n.align 3\n.byte 1\n", Some("ea000001")),
    ("q = 5\nq2 := 6\n.byte q, q2\n", Some("0506")),
    ("q .set 1\n.byte q\nq .set 2\n.byte q\n", Some("0102")),
    ("l: .byte 1\n.word l\n", Some("010000")),
    ("l: \nnop\n.word l\n", Some("ea0000")),
    ("@c: .byte 1\n.word @c\n", Some("010000")),
    ("q2 := *\n.word q2\n", Some("0000")),
    (".word *\n", Some("0000")),
    // `*` in a list is the address of the value it is in.
    (".word *, *+2, *\n", Some("000004000400")),
    (".dbyt *, *\n", Some("00000002")),
    (".org $1000\n.word *,*+2\n", Some("00100410")),
    // A label on `.end`'s line or `.macro`'s is defined, and nothing after
    // `.end` is read, not even on its own line.
    ("nop\nfinish: .end\n", Some("ea")),
    (".word finish\nfinish: .end\n", Some("0200")),
    ("nop\n.end $100000000\n", Some("ea")),
    ("nop\n.end\n.byte '\n", Some("ea")),
    ("nop\n.end\nfoo bar\n", Some("ea")),
    (
        ".word entry\nentry: .macro emit\nnop\n.endmacro\nemit\n",
        Some("0200ea"),
    ),
    (
        ".dbg line\n.dbg LINE\n.dbg file, \"t.s\", 10, 0\nnop\n",
        Some("ea"),
    ),
    (".dbg bogus\n", None),
    (".dbg\n", None),
    // Settings that make no bytes, checked as ca65 checks them, and the
    // constructor tables, which export their name.
    (".setcpu \"6502\"\nnop\n", Some("ea")),
    (".setcpu \"6502\"\n.p02\nnop\n", Some("ea")),
    (".setcpu \"nope\"\n", None),
    (".setcpu 1\n", None),
    (".setcpu\n", None),
    (
        ".smart\n.smart +\n.smart -\n.smart on\n.smart OFF\nnop\n",
        Some("ea"),
    ),
    (".smart 1\n", None),
    (
        ".autoimport on\n.autoimport off\n.autoimport\nnop\n",
        Some("ea"),
    ),
    (".autoimport 1\n", None),
    (
        ".list\n.list +\n.list -\n.list ON\n.list Off\nnop\n",
        Some("ea"),
    ),
    (".list 1\n", None),
    (".debuginfo on\n.debuginfo +\nnop\n", Some("ea")),
    (".debuginfo 1\n", None),
    (
        ".listbytes 4\n.listbytes 255\n.listbytes unlimited\nnop\n",
        Some("ea"),
    ),
    (".listbytes 3\n", None),
    (".listbytes 256\n", None),
    (".listbytes UNLIMITED\n", None),
    (".listbytes fwd\nfwd=10\n", None),
    (
        ".pagelen 32\n.pagelength 127\n.pagelength unlimited\nnop\n",
        Some("ea"),
    ),
    (".pagelen 31\n", None),
    (".pagelen 128\n", None),
    (
        ".fileopt author, \"x\"\n.fopt Compiler, \"y\"\n.fileopt 3, \"z\"\nnop\n",
        Some("ea"),
    ),
    (".fileopt bogus, \"x\"\n", None),
    (".fileopt comment, 1\n", None),
    (".fileopt comment\n", None),
    (".fopt author, \"a\", \"b\"\n", None),
    (".fileopt 256, \"x\"\n", None),
    (".pushcpu\n.pushcpu\n.popcpu\n.popcpu\nnop\n", Some("ea")),
    (".condes init, 0\ninit: rts\n", Some("60")),
    (".condes init, 6, 1\ninit: rts\n", Some("60")),
    (".condes init, Constructor\ninit: rts\n", Some("60")),
    (
        ".constructor init\n.destructor done, 32\n.interruptor irq\ninit:\ndone:\nirq: rts\n",
        Some("60"),
    ),
    (".condes init, 7\ninit: rts\n", None),
    (".condes init, -1\ninit: rts\n", None),
    (".condes init, user\ninit: rts\n", None),
    (".condes init, 0, 33\ninit: rts\n", None),
    (".condes init, 0, 0\ninit: rts\n", None),
    (".constructor init, 0\ninit: rts\n", None),
    (".condes init, 0, prio\nprio=3\ninit: rts\n", None),
    // `.reloc` ends `.org`: addresses count from the segment again.
    ("nop\n.reloc\nnop\n", Some("eaea")),
    (
        ".org $10\nl: nop\n.reloc\nm: .word l, m\n",
        Some("ea10000100"),
    ),
    (".org $10\n.reloc\n.org $20\nl: .word l\n", Some("2000")),
    (".reloc 1\n", None),
    ("lda fwd\nfwd = $12\n", Some("ad1200")),
    ("fwd = $12\nlda fwd\n", Some("a512")),
    ("lab: nop\nlda lab\n", Some("eaad0000")),
    ("lda lab\nlab: nop\n", Some("ad0300ea")),
    (".org $10\nol: nop\nlda ol\n", Some("eaa510")),
    ("bne *\nbne lab\nlab:\n", Some("d0fed000")),
    ("q = 1000\nq2 = q - 990\nlda q2\n", Some("a50a")),
    // A made string keeps bytes its text cannot hold.
    (".asciiz .sprintf(\"%c%c\",200,1)\n", Some("c80100")),
    (".byte .sprintf(\"%c\",255), 1\n", Some("ff01")),
    // `.sizeof` a label: the bytes its line made, known now.
    ("a1: .byte 1,2,3\n.byte .sizeof(a1)\n", Some("01020303")),
    ("a1:\n.byte 1,2,3\n.byte .sizeof(a1)\n", Some("01020300")),
    ("a1: nop\nlda #1\n.byte .sizeof(a1)\n", Some("eaa90101")),
    ("a1: .res 3\n.byte .sizeof(a1)\n", Some("00000003")),
    (".byte .sizeof(a1)\na1: .word 1,2\n", None),
    ("v=5\n.byte .sizeof(v)\n", None),
    (".byte .sizeof(nosuch)\n", None),
    (".byte .sizeof(*)\n", None),
    ("a1: .byte 1\n.byte .SIZEOF(a1)\n", Some("0101")),
    // A line's label is defined before the rest of the line is read.
    (".org $10\nl: lda l\n", Some("a510")),
    ("q: .byte .defined(q),.const(q)\n", Some("0100")),
    (".org 2\nl: .byte .const(l)\n", Some("01")),
    ("l: .byte .const(l)\n", Some("00")),
    // `.defined` and `.referenced` do not read the name; `.const` and a
    // plain use do, in the order ca65 reads them.
    (".byte .defined(v),.ref(v)\nv=1\n", Some("0000")),
    ("v=1\n.byte .ref(v),.defined(v),.ref(v)\n", Some("000100")),
    (
        "v=1\n.byte .def(v),.ref(v),.referenced(v)\n",
        Some("010000"),
    ),
    (".byte .ref(v)\n.byte v\nv=1\n", Some("0001")),
    (".byte .ref(v),v,.ref(v)\nv=1\n", Some("000101")),
    (".byte .defined(v),.const(v)\nv=1\n", Some("0000")),
    (
        "v=1\n.byte .const(v),.const(v+1),.const(*),.const(w)\nw:\n",
        Some("01010000"),
    ),
    (".byte .const(3)\n", Some("01")),
    ("q=1\n.byte .const(q),.ref(q)\n", Some("0101")),
    (
        "VALUE=2\n.byte .ref(VALUE),.const(VALUE),.ref(VALUE)\n",
        Some("000101"),
    ),
    ("l: .byte .ref(l),.ref(l)\n", Some("0000")),
    (
        "v=1\n.if .ref(v)\n.byte 1\n.endif\n.ifref v\n.byte 2\n.endif\n",
        Some(""),
    ),
    (
        "v=1\n.if .const(v)\n.byte 1\n.endif\n.ifref v\n.byte 2\n.endif\n",
        Some("0102"),
    ),
    (".byte .defined(1)\n", None),
    (".byte .ref(1+v)\n", None),
    (".byte .const(1,2)\n", None),
    // Messages and assertions.
    (".out \"hi\"\n.byte 1\n", Some("01")),
    (".warning \"w\"\n.byte 1\n", Some("01")),
    (".error \"e\"\n.byte 1\n", None),
    (".fatal \"f\"\n.byte 1\n", None),
    (".out 1\n", None),
    (".out\n", None),
    (".out \"a\",\"b\"\n", None),
    (".out .concat(\"a\",\"b\")\n", Some("")),
    (".if 0\n.error \"e\"\n.endif\n.byte 1\n", Some("01")),
    (".warning \"a\", \"b\"\n.byte 1\n", Some("01")),
    (".warning 1\n", None),
    (".warning .sprintf(\"%c\",255)\n.byte 1\n", Some("01")),
    (".assert 1, error, \"x\"\n.byte 1\n", Some("01")),
    (".assert 0, error, \"x\"\n.byte 1\n", None),
    (".assert 0, warning, \"x\"\n.byte 1\n", Some("01")),
    (".assert 0, error\n.byte 1\n", None),
    (".assert 0, ldwarning, \"x\"\n.byte 1\n", Some("01")),
    (".assert 0, lderror, \"x\"\n.byte 1\n", None),
    (".assert 1, WARNING, \"a\"\n.byte 1\n", Some("01")),
    (".assert l=2, error, \"x\"\n.byte 1\nl:\n", None),
    (".assert l=1, error, \"x\"\n.byte 1\nl:\n", Some("01")),
    (".assert 2, error\n", Some("")),
    (".assert 0, bogus\n", None),
    (".assert 0\n", None),
    (".assert 0, warning, 1\n.byte 1\n", None),
    (".warning \"a\" junk\n.byte 1\n", Some("01")),
    (".out \"a\" junk\n.byte 1\n", None),
    (".assert 0, warning, \"a\" junk\n.byte 1\n", None),
    // A token function runs where ca65 reads it, so a `.ref` before it on
    // the line does not count the names it reads, and one after it does.
    // A plain read before the `.ref` counts. (lang198x/ca65#52)
    (
        "V=2\n.byte .ref(V),.sprintf(\"%d\",V),.ref(V)\n",
        Some("003201"),
    ),
    (
        "V=2\n.byte .ref(V)+.strlen(.sprintf(\"%d\",V)),.ref(V)\n",
        Some("0101"),
    ),
    (
        "V=2\nW=3\n.byte .sprintf(\"%d\",V),.ref(V),.ref(W),.sprintf(\"%d\",W),.ref(W)\n",
        Some("3201003301"),
    ),
    (
        "V=2\n.byte .ref(V),.tcount({.sprintf(\"%d\",V)}),.ref(V)\n",
        Some("000101"),
    ),
    (
        "V=2\n.byte .tcount({.ref(V) .sprintf(\"%d\",V)}),.ref(V)\n",
        Some("0501"),
    ),
    (
        "V=2\n.byte .sprintf(\"%d%d\",.ref(V),V),.ref(V)\n",
        Some("303201"),
    ),
    ("V=2\n.byte V,.ref(V),.sprintf(\"%d\",V)\n", Some("020132")),
    (
        "V=2\n.byte .referenced(V),.ref(V),V,.ref(V)\n",
        Some("00000201"),
    ),
    ("V=2\n.byte .ref(V),.left(1,{V}),.ref(V)\n", Some("000201")),
    (
        "V=2\n.byte .ref(V),.strlen(.string(V)),.ref(V)\n",
        Some("000100"),
    ),
    (
        ".byte .ref(U),.sprintf(\"%d\",.defined(U)),.ref(U)\n",
        Some("003000"),
    ),
    (
        "V=2\n.if .ref(V) .or .strlen(.sprintf(\"%d\",V))=9\n.byte 1\n.else\n.byte 2\n.endif\n",
        Some("02"),
    ),
    (
        "V=2\n.if .ref(V)+V+.strlen(.sprintf(\"%d\",V))=3\n.byte 1\n.else\n.byte 2\n.endif\n",
        Some("01"),
    ),
    (
        "V=2\n.if V+.ref(V)+.strlen(.sprintf(\"%d\",V))=4\n.byte 1\n.else\n.byte 2\n.endif\n",
        Some("01"),
    ),
    (
        "V=2\n.repeat .strlen(.sprintf(\"%d\",V))\nnop\n.endrepeat\n.byte .ref(V)\n",
        Some("ea01"),
    ),
    (
        "V=2\n.macro m\n.byte .ref(V),.sprintf(\"%d\",V),.ref(V)\n.endmacro\nm\n",
        Some("003201"),
    ),
    // ca65 defines a line's label before it runs the line's functions.
    (
        "H: .byte .sprintf(\"%d\",.defined(H)),.const(H-H)\n",
        Some("3101"),
    ),
    (": .byte .sprintf(\"%d\", 1)\n.word :-\n", Some("310000")),
    (".ident(\"q\"): .byte .defined(q)\n", Some("01")),
    // The rest of a labelled line is read without its label, even when
    // `.end` leaves no function to run.
    (
        ".define NAME finish\n.word finish\nNAME:.end .sprintf(\"%d\",4294967300)\n.byte 8\n",
        Some("0200"),
    ),
];

#[test]
fn every_measured_program_matches_ca65() {
    for &(source, hex) in MEASURED {
        let got = assemble(source);
        assert_eq!(got.as_deref().ok(), hex, "{source:?}: {got:?}");
    }
}

/// The items `source`'s first line lowers to: each label, and whether an
/// operation came with it.
fn items(source: &str) -> Vec<(Option<String>, bool)> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
        .expect("lowers")
        .items
        .into_iter()
        .map(|item| (item.label, item.op.is_some()))
        .collect()
}

#[test]
fn labels_are_items_of_their_own_or_carry_the_operation() {
    let named = |n: &str| Some(n.to_owned());
    assert_eq!(
        items(": l2: nop\n"),
        [(named(":"), false), (named("l2"), true)]
    );
    assert_eq!(items("@near:\n"), [(named("@near"), false)]);
    assert_eq!(items("nop\n"), [(None, true)]);
    assert_eq!(items("\n"), []);
    // A label before a macro call reaches the host on its own.
    assert_eq!(
        items(".macro m\nnop\n.endmacro\nl3: m\n"),
        [(named("l3"), false)]
    );
}

/// A line's items, as `items` gives them, and its change to the scopes.
type Reported = (Vec<(Option<String>, bool)>, Option<Scope>);

/// The change to the scopes `source`'s first line makes, and its items.
fn scope(source: &str) -> Result<Reported, String> {
    scope_on(source, Profile::CA65_2_19_6502)
}

/// As [`scope`], for `profile`'s CPU.
fn scope_on(source: &str, profile: Profile) -> Result<Reported, String> {
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    let lowered = line(&active, profile, &mini).map_err(|d| d.message().to_owned())?;
    let items = lowered
        .items
        .into_iter()
        .map(|item| (item.label, item.op.is_some()))
        .collect();
    Ok((items, lowered.scope))
}

#[test]
fn scope_commands_are_reported_after_the_lines_items() {
    let open = |name: Option<&str>, procedure| Scope::Open {
        name: name.map(str::to_owned),
        procedure,
        size: None,
    };
    let label = |n: &str| (Some(n.to_owned()), false);
    assert_eq!(
        scope(".scope inner\n"),
        Ok((vec![], Some(open(Some("inner"), false))))
    );
    assert_eq!(scope(".SCOPE\n"), Ok((vec![], Some(open(None, false)))));
    // `.proc` also defines its name, in the scope around it.
    assert_eq!(
        scope(".proc main\n"),
        Ok((vec![label("main")], Some(open(Some("main"), true))))
    );
    // An unnamed `.proc` is an unnamed `.scope`.
    assert_eq!(scope(".proc\n"), Ok((vec![], Some(open(None, false)))));
    // ca65 takes an unnamed label before either.
    assert_eq!(
        scope(": .proc p\n"),
        Ok((vec![label(":"), label("p")], Some(open(Some("p"), true))))
    );
    // A label on the closing line belongs to the scope that closes.
    assert_eq!(
        scope("tail: .endscope\n"),
        Ok((vec![label("tail")], Some(Scope::Close { procedure: false })))
    );
    assert_eq!(
        scope(".endproc\n"),
        Ok((vec![], Some(Scope::Close { procedure: true })))
    );
    assert_eq!(scope("nop\n"), Ok((vec![(None, true)], None)));
    // A named `.proc` carries the size written on it, which is the size of
    // its name. ca65 accepts one on `.scope`, where it changes nothing.
    let on = |source: &str, cpu| match scope_on(source, Profile::CA65_2_19_6502.with_cpu(cpu)) {
        Ok((_, Some(Scope::Open { size, .. }))) => size,
        other => panic!("{source}: {other:?}"),
    };
    let sized = |source: &str| on(source, crate::Cpu::Mos6502);
    assert_eq!(sized(".proc p: zp\n"), Some(Size::Zero));
    assert_eq!(sized(".proc p: direct\n"), Some(Size::Zero));
    assert_eq!(sized(".proc p: abs\n"), Some(Size::Absolute));
    assert_eq!(on(".proc p: far\n", crate::Cpu::Wdc65816), Some(Size::Far));
    assert_eq!(sized(".scope s: zp\n"), None);
    assert_eq!(sized(".proc p\n"), None);
    // `far` only on the 65816, as ca65 checks it.
    assert!(scope(".proc p: far\n").is_err());
}

/// The change to the segment `source`'s first line makes.
fn segment(source: &str) -> Result<Option<Segment>, String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
        .map(|lowered| lowered.segment)
        .map_err(|d| d.message().to_owned())
}

#[test]
fn segment_commands_are_reported() {
    let used = |name: &str, size| {
        Ok(Some(Segment::Use {
            name: name.to_owned(),
            size,
        }))
    };
    assert_eq!(segment(".segment \"CODE\"\n"), used("CODE", None));
    // Names are case-sensitive, as ld65 reads them.
    assert_eq!(segment(".segment \"code\"\n"), used("code", None));
    assert_eq!(
        segment(".segment \"ZP2\": zeropage\n"),
        used("ZP2", Some(Size::Zero))
    );
    assert_eq!(
        segment(".segment \"A\": abs\n"),
        used("A", Some(Size::Absolute))
    );
    assert_eq!(segment(".segment \"F\": far\n"), used("F", Some(Size::Far)));
    for (word, name) in [
        (".code", "CODE"),
        (".RODATA", "RODATA"),
        (".data", "DATA"),
        (".bss", "BSS"),
        (".zeropage", "ZEROPAGE"),
        (".null", "NULL"),
    ] {
        assert_eq!(segment(&format!("{word}\n")), used(name, None), "{word}");
    }
    assert_eq!(segment(".pushseg\n"), Ok(Some(Segment::Push)));
    assert_eq!(segment(".popseg\n"), Ok(Some(Segment::Pop)));
    assert_eq!(segment("nop\n"), Ok(None));
    assert!(segment(".segment 5\n").is_err());
}

/// The first line of `source`: what it declares, and its labels.
fn declared(source: &str) -> Result<(Option<Visibility>, Vec<Option<String>>), String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
        .map(|lowered| {
            let labels = lowered.items.into_iter().map(|item| item.label).collect();
            (lowered.visibility, labels)
        })
        .map_err(|d| d.message().to_owned())
}

#[test]
fn declarations_are_reported() {
    let names = |list: &[(&str, Option<Size>)]| -> Vec<Declared> {
        list.iter()
            .map(|(name, size)| Declared {
                name: (*name).to_owned(),
                size: *size,
            })
            .collect()
    };
    let seen = |source: &str| declared(source).map(|(v, _)| v.map(|v| (v.kind, v.names)));
    assert_eq!(
        seen(".import v, w\n"),
        Ok(Some((Visible::Import, names(&[("v", None), ("w", None)]))))
    );
    assert_eq!(
        seen(".importzp v\n"),
        Ok(Some((Visible::Import, names(&[("v", Some(Size::Zero))]))))
    );
    assert_eq!(
        seen(".import v: zp, w: abs\n"),
        Ok(Some((
            Visible::Import,
            names(&[("v", Some(Size::Zero)), ("w", Some(Size::Absolute))])
        )))
    );
    assert_eq!(
        seen(".EXPORTZP v\n"),
        Ok(Some((Visible::Export, names(&[("v", Some(Size::Zero))]))))
    );
    assert_eq!(
        seen(".global v\n"),
        Ok(Some((Visible::Global, names(&[("v", None)]))))
    );
    assert_eq!(
        seen(".globalzp v\n"),
        Ok(Some((Visible::Global, names(&[("v", Some(Size::Zero))]))))
    );
    assert_eq!(
        seen(".forceimport v\n"),
        Ok(Some((Visible::ForceImport, names(&[("v", None)]))))
    );
    assert_eq!(seen("nop\n"), Ok(None));
    // An export with a value also defines the name.
    assert_eq!(
        declared(".export v := 3, w, q = 4\n").map(|(_, labels)| labels),
        Ok(vec![Some("v".to_owned()), Some("q".to_owned())])
    );
    for refused in [
        ".import v := 3\n",
        ".global v := 3\n",
        ".export v: far\n",
        ".import v: bogus\n",
        ".export s::v\n",
        ".import ::v\n",
    ] {
        assert!(declared(refused).is_err(), "{refused:?}");
    }
}

/// What `source` asks to put here from a file, at its first `.incbin`.
/// Lines before it only define names.
fn binary(source: &str) -> Result<Option<Binary>, String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    while let Some(active) = expander.next_line(&mut mini) {
        let lowered = line(&active, profile, &mini).map_err(|d| d.message().to_owned())?;
        if lowered.binary.is_some() {
            return Ok(lowered.binary);
        }
        for item in lowered.items {
            if let (Some(label), Some(Operation::Equ(value))) = (item.label, item.op)
                && let Some(v) = mini.value(&value)
            {
                mini.known.insert(label, Value::Constant(v));
            }
        }
    }
    Ok(None)
}

#[test]
fn a_binary_file_is_reported() {
    let file = |name: &str, offset, size| {
        Ok(Some(Binary {
            name: name.to_owned(),
            offset,
            size,
        }))
    };
    assert_eq!(binary(".incbin \"a.bin\"\n"), file("a.bin", None, None));
    assert_eq!(
        binary(".incbin \"a.bin\", 2\n"),
        file("a.bin", Some(2), None)
    );
    assert_eq!(
        binary(".incbin \"a.bin\", 2, -2\n"),
        file("a.bin", Some(2), Some(-2))
    );
    assert_eq!(
        binary("n=1\n.incbin \"a.bin\", n, n\n"),
        file("a.bin", Some(1), Some(1))
    );
    assert_eq!(
        binary(".incbin .concat(\"a\",\".bin\"), 7\n"),
        file("a.bin", Some(7), None)
    );
    assert_eq!(binary("nop\n"), Ok(None));
    for refused in [
        ".incbin \"a.bin\", n\nn=1\n",
        ".incbin \"a.bin\", 1, 1, 1\n",
        ".incbin\n",
        ".incbin 5\n",
    ] {
        assert!(binary(refused).is_err(), "{refused:?}");
    }
}

#[test]
fn ca65_refuses_a_label_before_a_scope() {
    let profile = Profile::CA65_2_19_6502;
    for source in ["x: .scope s\n", "x: .proc p\n"] {
        let mut expander = Expander::new(source, profile).expect("parses");
        let _ = expander.next_line(&mut Mini::default());
        assert!(!expander.diagnostics().is_empty(), "{source:?}");
    }
}

#[test]
fn what_is_not_lowered_yet_says_so() {
    let profile = Profile::CA65_2_19_6502;
    for source in [".byte .bank(1)\n", ".segment \"X\": dword\n"] {
        let mut expander = Expander::new(source, profile).expect("parses");
        let mut mini = Mini::default();
        let active = expander.next_line(&mut mini).expect("a line");
        let problem = line(&active, profile, &mini)
            .err()
            .unwrap_or_else(|| panic!("{source:?} lowers"));
        assert_eq!(problem.kind(), DiagnosticKind::Unsupported, "{source:?}");
    }
}

/// The first line of `source`, lowered.
fn first(source: &str) -> Result<super::Lowered, lang198x_syntax::Diagnostic> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
}

#[test]
fn settings_report_the_cpu_stack_and_constructors_export() {
    use crate::lower::CpuStack;
    assert_eq!(
        first(".pushcpu\n").ok().and_then(|l| l.cpu),
        Some(CpuStack::Push)
    );
    assert_eq!(
        first(".POPCPU\n").ok().and_then(|l| l.cpu),
        Some(CpuStack::Pop)
    );
    assert_eq!(first(".p02\n").ok().and_then(|l| l.cpu), None);
    let exported = |source: &str| {
        first(source)
            .ok()
            .and_then(|l| l.visibility)
            .map(|v| (v.kind, v.names))
    };
    let init = vec![Declared {
        name: "init".to_owned(),
        size: None,
    }];
    assert_eq!(
        exported(".condes init, 0\n"),
        Some((Visible::Export, init.clone()))
    );
    assert_eq!(
        exported(".constructor init, 5\n"),
        Some((Visible::Export, init))
    );
    assert!(first(".condes s::init, 0\n").is_err());

    // What is not lowered yet is not the same as refused: ca65's `none`, a
    // CPU with no instructions, and some debug information.
    for source in [".setcpu \"none\"\n", ".dbg file, \"t.c\", 1 + 1, 0\n"] {
        let problem = first(source).err().expect(source);
        assert_eq!(problem.kind(), DiagnosticKind::Unsupported, "{source}");
    }
    let problem = first(".setcpu \"nope\"\n").err().expect("unknown");
    assert_eq!(problem.kind(), DiagnosticKind::Invalid);
    // Every CPU is lowered.
    for source in [
        ".setcpu \"sweet16\"\n",
        ".psweet16\n",
        ".setcpu \"m740\"\n",
        ".p45gs02\n",
        ".setcpu \"65C02\"\n",
        ".pc02\n",
        ".p02x\n",
        ".pce02\n",
        ".setcpu \"huc6280\"\n",
        ".p816\n",
        ".pdtv\n",
    ] {
        assert!(first(source).is_ok(), "{source}");
    }
}

/// Each line of `source` that runs, lowered, as its record change or its
/// error.
fn records(source: &str) -> Vec<Result<Option<crate::lower::Record>, String>> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let mut out = Vec::new();
    while let Some(active) = expander.next_line(&mut mini) {
        out.push(
            line(&active, profile, &mini)
                .map(|l| l.record)
                .map_err(|d| d.message().to_owned()),
        );
    }
    out
}

/// Records as ca65 V2.19 reads them, measured: storage sizes and counts,
/// `.res`, `.org` and `.tag` in a struct, enum members, nested records, and
/// `.tag` in the program.
#[test]
fn records_report_each_line() {
    use crate::lower::{Record, RecordKind};
    let field = |name: Option<&str>, bytes| {
        Ok(Some(Record::Field {
            name: name.map(str::to_owned),
            bytes,
        }))
    };
    assert_eq!(
        records(
            ".struct P\nf1 .byte\nf2 .word\nf3 .dword\nf4 .faraddr\nf5 .dbyt\nf6 .addr\n.byte 3\nf7 .res 2\nf8 .word 2\n.endstruct\n"
        ),
        [
            Ok(Some(Record::Open {
                kind: RecordKind::Struct,
                name: Some("P".to_owned()),
                field: None
            })),
            field(Some("f1"), 1),
            field(Some("f2"), 2),
            field(Some("f3"), 4),
            field(Some("f4"), 3),
            field(Some("f5"), 2),
            field(Some("f6"), 2),
            field(None, 3),
            field(Some("f7"), 2),
            field(Some("f8"), 4),
            Ok(Some(Record::Close(RecordKind::Struct))),
        ]
    );
    assert_eq!(
        records(".union\nf .org 10\nt .tag Q 3\ng .struct\n.endstruct\n.endunion\n"),
        [
            Ok(Some(Record::Open {
                kind: RecordKind::Union,
                name: None,
                field: None
            })),
            Ok(Some(Record::Offset {
                name: Some("f".to_owned()),
                offset: 10
            })),
            Ok(Some(Record::Tag {
                name: Some("t".to_owned()),
                record: "Q".to_owned(),
                count: 3
            })),
            Ok(Some(Record::Open {
                kind: RecordKind::Struct,
                name: None,
                field: Some("g".to_owned())
            })),
            Ok(Some(Record::Close(RecordKind::Struct))),
            Ok(Some(Record::Close(RecordKind::Union))),
        ]
    );
    // A member named with `.org` is where `.org` puts the next one.
    assert_eq!(
        records(".struct P\nf1 .org 3\n")[1],
        Ok(Some(Record::Offset {
            name: Some("f1".to_owned()),
            offset: 3
        }))
    );
    let enumerators = records(".enum E\ne1\ne2 = 10\n.endenum\n");
    assert_eq!(
        enumerators[1],
        Ok(Some(Record::Enumerator {
            name: "e1".to_owned(),
            value: None
        }))
    );
    assert_eq!(
        enumerators[2],
        Ok(Some(Record::Enumerator {
            name: "e2".to_owned(),
            value: Some(asm198x_ir::Expr::Num(10))
        }))
    );
    assert_eq!(
        records("p: .tag Q, 2\n.tag Q\n"),
        [
            Ok(Some(Record::Reserve {
                record: "Q".to_owned(),
                count: 2
            })),
            Ok(Some(Record::Reserve {
                record: "Q".to_owned(),
                count: 1
            })),
        ]
    );
    for (source, message) in [
        (".struct P\nf1 .byte 0\n", "Range error"),
        (".struct P\nf1 .byte -1\n", "Range error"),
        (".struct P\nf1 .res 0\n", "Range error"),
        (".struct P\n.res\n", "Expected a byte count"),
        (".struct P\n.org\n", "Expected an address"),
        (".struct P\n.org $1000000\n", "Range error"),
        (".struct P\nf1 .res $1000000\n", "Range error"),
        (".struct P\nf1 .word K\n", "Constant expression expected"),
        (".tag Q, 0\n", "Range error"),
        (
            ".struct P\nf1 .segment \"X\"\n",
            "Expected a storage allocator after the field name",
        ),
        (".tag 3\n", "Expected an identifier"),
    ] {
        assert_eq!(
            records(source).last().cloned(),
            Some(Err(message.to_owned())),
            "{source}"
        );
    }
}

/// A macro's lines read as members where the macro is called inside a
/// record, as in ca65 V2.19, which reads them in the record.
#[test]
fn a_macro_makes_members_inside_a_record() {
    use crate::lower::Record;
    let found = records(".macro two\nm1 .byte\nm2 .word\n.endmacro\n.struct S\ntwo\n.endstruct\n");
    assert!(
        found.contains(&Ok(Some(Record::Field {
            name: Some("m2".to_owned()),
            bytes: 2
        }))),
        "{found:?}"
    );
    let found = records(".macro two\nm1\nm2 = 4\n.endmacro\n.enum E\ntwo\n.endenum\nm3 = 1\n");
    assert!(
        found.contains(&Ok(Some(Record::Enumerator {
            name: "m1".to_owned(),
            value: None
        }))),
        "{found:?}"
    );
    // After the record, the lines read as they would anywhere.
    assert_eq!(found.last(), Some(&Ok(None)));
}

/// What each line of `source` makes that shows the 65816's widths: an
/// instruction's mode, or the values of a `.byte`, in order. An equate's
/// value is known from its line on.
fn widths_shown(source: &str) -> Vec<String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let mut shown = Vec::new();
    while let Some(active) = expander.next_line(&mut mini) {
        let lowered =
            line(&active, profile, &mini).unwrap_or_else(|d| panic!("{source:?}: {}", d.message()));
        for item in lowered.items {
            match (item.label, item.op) {
                (Some(label), Some(Operation::Equ(value))) => {
                    let value = mini.value(&value).expect("a constant");
                    mini.known.insert(label, Value::Constant(value));
                }
                (_, Some(Operation::Instruction { mode, .. })) => shown.push(mode.to_owned()),
                (_, Some(Operation::Sized { values, .. })) => shown.extend(
                    values
                        .iter()
                        .map(|v| mini.value(v).expect("a constant").to_string()),
                ),
                _ => {}
            }
        }
    }
    shown
}

#[test]
fn the_65816s_widths_size_its_immediates() {
    // Measured on ca65 V2.19, as the bytes of each line.
    let cases: &[(&str, &[&str])] = &[
        (
            // a9 01, a9 01 00, a2 01, a2 01 00, a9 01
            ".p816\nlda #1\n.a16\nlda #1\nldx #1\n.i16\nldx #1\n.a8\nlda #1\n",
            &[
                "immediate",
                "immediate16",
                "immediate",
                "immediate16",
                "immediate",
            ],
        ),
        (
            // c2 30, a9 01 00, a2 01 00, e2 20, a9 01, a2 01 00
            ".p816\n.smart\nrep #$30\nlda #1\nldx #1\nsep #$20\nlda #1\nldx #1\n",
            &[
                "immediate",
                "immediate16",
                "immediate16",
                "immediate",
                "immediate",
                "immediate16",
            ],
        ),
        // Without `.smart`, `rep` changes nothing: c2 30, a9 01.
        (".p816\nrep #$30\nlda #1\n", &["immediate", "immediate"]),
        // e2 30, c2 30, a9 01.
        (
            ".p816\n.smart +\nsep #$30\n.smart -\nrep #$30\nlda #1\n",
            &["immediate", "immediate", "immediate"],
        ),
        // A value not known yet changes nothing: c2 20, a9 01.
        (
            ".p816\n.smart\nrep #v\nv = $20\nlda #1\n",
            &["immediate", "immediate"],
        ),
        // c2 20, a9 01 00.
        (
            ".p816\n.smart\nv = $20\nrep #v\nlda #1\n",
            &["immediate", "immediate16"],
        ),
        // The widths outlast another CPU, which ignores them: a9 01, a9 01 00.
        (
            ".p816\n.a16\n.p02\nlda #1\n.p816\nlda #1\n",
            &["immediate", "immediate16"],
        ),
        // `.asize` and `.isize` are 8 but on the 65816: 10 08 08.
        (
            ".p816\n.a16\n.byte .asize, .isize\n.p02\n.byte .asize\n",
            &["16", "8", "8"],
        ),
    ];
    for (source, want) in cases {
        assert_eq!(widths_shown(source), *want, "{source:?}");
    }
}

#[test]
fn the_width_commands_are_only_for_the_65816() {
    for source in [".a8\n", ".a16\n", ".i8\n", ".i16\n"] {
        let problem = first(source).err().expect(source);
        assert_eq!(problem.message(), "Command is only valid in 65816 mode");
    }
}

#[test]
fn the_65816s_addr_keeps_the_low_word() {
    // Measured on ca65 V2.19: ff ff 01 00 on the 65816, and a range error
    // on the 6502.
    let (bytes, _) = pass(".p816\n.addr -65537, 65537\n", HashMap::new()).expect("assembles");
    assert_eq!(bytes, [0xFF, 0xFF, 0x01, 0x00]);
    assert!(pass(".addr 65537\n", HashMap::new()).is_err());
}

/// `.cpu` asks the line which CPU is in force, and `.time` asks the
/// assembler, so the two answer through a whole line too.
#[test]
fn a_line_knows_its_cpu_and_the_time() {
    let source = ".setcpu \"65c02\"\n.word .cpu, .time & 0\n";
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let _setcpu = expander.next_line(&mut mini).expect("a line");
    let active = expander.next_line(&mut mini).expect("a line");
    let Ok(lowered) = line(&active, profile, &mini) else {
        panic!("the line lowers")
    };
    let Some(Operation::Sized { values, .. }) = lowered.items.into_iter().find_map(|item| item.op)
    else {
        panic!("words")
    };
    let values: Vec<_> = values.iter().map(|v| mini.value(v)).collect();
    assert_eq!(values, [Some(0x0032), Some(0)]);
}

/// The character map turns the characters of strings and character
/// constants into other bytes, but not a `.literal` string, as ca65 V2.19
/// does. `.strat`, `.if` and an instruction's operand see it too.
#[test]
fn the_character_map_translates_characters() {
    let assembled = |source: &str| pass(source, HashMap::new()).map(|(bytes, _)| bytes);
    assert_eq!(
        assembled(
            ".charmap $41, $05\n.byte \"AB\", 'A'\n lda #'A'\n.asciiz \"A\"\n\
             .literal \"AB\", $41, 'A'\n.byte .strat(\"A\", 0)\n.word 'A'+1\n"
        ),
        Ok(vec![
            5, 0x42, 5, 0xA9, 5, 5, 0, 0x41, 0x42, 0x41, 5, 5, 6, 0
        ])
    );
    assert_eq!(
        assembled(".charmap 'A', 'B'\n.if 'A' = 'B'\n.byte 1\n.endif\n"),
        Ok(vec![1])
    );
    // A map kept and taken back.
    assert_eq!(
        assembled(
            ".charmap $41, 1\n.pushcharmap\n.byte \"A\"\n.charmap $41, 2\n.byte \"A\"\n\
             .popcharmap\n.byte \"A\"\n"
        ),
        Ok(vec![1, 2, 1])
    );
    // ca65's errors.
    for (source, error) in [
        (".charmap $41\n", "takes a character and its byte"),
        (".charmap 1, 2, 3\n", "takes a character and its byte"),
        (".charmap 256, 1\n", "Index must be in byte range"),
        (
            ".charmap 65, 256\n",
            "Replacement character code must be in byte range",
        ),
        (
            ".charmap later, 1\nlater = 2\n",
            "Constant expression expected",
        ),
        (".pushcharmap 1\n", "expected the end of the line"),
    ] {
        let problem = assembled(source).expect_err(source);
        assert!(problem.contains(error), "{source:?}: {problem}");
    }
}

/// What `.sprintf` writes for a conversion is raw: ca65's character map
/// leaves it alone, while the format's own characters, and a string
/// `.concat` joins on, go through the map.
#[test]
fn a_conversion_is_not_translated() {
    let assembled = |body: &str| {
        let source = format!(".charmap $41, $05\n.charmap $31, $09\n.charmap $78, $08\n{body}\n");
        pass(&source, HashMap::new()).map(|(bytes, _)| bytes)
    };
    assert_eq!(
        assembled(".byte .sprintf(\"x%cy\", $41)"),
        Ok(vec![8, 0x41, 0x79])
    );
    assert_eq!(assembled(".byte .sprintf(\"%d\", 1)"), Ok(vec![0x31]));
    assert_eq!(
        assembled(".byte .concat(.sprintf(\"%s\", \"A\"), \"A\")"),
        Ok(vec![0x41, 5])
    );
    assert_eq!(
        assembled(".asciiz .sprintf(\"A%sA\", \"A\")"),
        Ok(vec![5, 0x41, 5, 0])
    );
    // A byte above 127 is kept as it was.
    assert_eq!(assembled(".byte .sprintf(\"%c\", $C1)"), Ok(vec![0xC1]));
    // `.strat` goes through the map whatever the byte.
    assert_eq!(
        assembled(".byte .strat(.sprintf(\"%s\", \"A\"), 0)"),
        Ok(vec![5])
    );
}

/// The `.dbg` lines a C compiler writes, read as ca65 V2.19 reads them, with
/// its errors. What the file table and scopes decide is the assembler's.
#[test]
fn debug_information_is_reported() {
    use crate::lower::Dbg;
    let dbg = |source: &str| {
        first(source)
            .map(|l| l.dbg)
            .map_err(|d| d.message().to_owned())
    };
    assert_eq!(
        dbg(".dbg file, \"t.c\", 150, 1793615259\n"),
        Ok(Some(Dbg::File {
            name: "t.c".to_owned()
        }))
    );
    assert_eq!(
        dbg(".dbg line, \"t.c\", 2\n"),
        Ok(Some(Dbg::Line {
            file: "t.c".to_owned()
        }))
    );
    assert_eq!(dbg(".dbg line\n"), Ok(Some(Dbg::EndLine)));
    assert_eq!(
        dbg(".dbg func, \"total\", \"00\", static, \"_total\"\n"),
        Ok(Some(Dbg::Func {
            asm_name: "_total".to_owned()
        }))
    );
    assert_eq!(
        dbg(".dbg sym, \"r\", \"00\", auto, -2\n"),
        Ok(Some(Dbg::Sym { asm_name: None }))
    );
    assert_eq!(
        dbg(".dbg sym, \"c\", \"0a\", static, \"_c\"\n"),
        Ok(Some(Dbg::Sym {
            asm_name: Some("_c".to_owned())
        }))
    );
    assert_eq!(
        dbg(".dbg sym, \"r\", \"00\", register, \"regbank\", 4\n"),
        Ok(Some(Dbg::Sym {
            asm_name: Some("regbank".to_owned())
        }))
    );
    assert_eq!(dbg("nop\n"), Ok(None));
    for (source, error) in [
        (".dbg\n", "Expected an identifier"),
        (".dbg 1\n", "Expected an identifier"),
        (".dbg zz\n", "Expected FILE, FUNC, LINE or SYM"),
        (".dbg file \"t.c\", 1, 2\n", "Expected `,`"),
        (".dbg file, 1, 1, 2\n", "String constant expected"),
        (
            ".dbg line, \"t.c\", -1\n",
            "Line number is out of valid range",
        ),
        (
            ".dbg func, \"f\", \"0\", static, \"_f\"\n",
            "Type value has invalid length",
        ),
        (
            ".dbg func, \"f\", \"0g\", static, \"_f\"\n",
            "Type value contains invalid characters",
        ),
        (
            ".dbg func, \"f\", \"00\", auto, \"_f\"\n",
            "Storage class specifier expected",
        ),
        (
            ".dbg sym, \"x\", \"00\", 1, 2\n",
            "Storage class specifier expected",
        ),
        (".dbg line, \"t.c\", 2, 3\n", "Expected the end of the line"),
    ] {
        let problem = dbg(source).expect_err(source);
        assert!(problem.contains(error), "{source:?}: {problem}");
    }
}

/// An address size on `.proc` or `.scope`, which a C compiler writes on
/// every function, is checked as ca65 V2.19 checks it.
#[test]
fn a_procedure_may_name_its_size() {
    let opened = |source: &str| {
        first(source)
            .map(|l| l.scope.is_some())
            .map_err(|d| d.kind())
    };
    for size in ["near", "abs", "absolute", "NEAR"] {
        assert_eq!(
            opened(&format!(".proc _main: {size}\n")),
            Ok(true),
            "{size}"
        );
        assert_eq!(opened(&format!(".scope s: {size}\n")), Ok(true), "{size}");
    }
    for (size, kind) in [
        ("far", DiagnosticKind::Invalid),
        ("long", DiagnosticKind::Invalid),
        ("dword", DiagnosticKind::Invalid),
        ("dp", DiagnosticKind::Invalid),
    ] {
        assert_eq!(opened(&format!(".proc p: {size}\n")), Err(kind), "{size}");
    }
    assert_eq!(opened(".proc p: zeropage\n"), Ok(true));
    // The 65816 has far addresses.
    let profile = Profile::CA65_2_19_6502.with_cpu(crate::Cpu::Wdc65816);
    let mut expander = Expander::new(".proc p: far\n", profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    assert!(line(&active, profile, &mini).is_ok_and(|l| l.scope.is_some()));
}

/// `.case on`, which a C compiler writes, is what ca65 does anyway.
#[test]
fn names_keep_their_case() {
    for source in [
        ".case on\n",
        ".case +\n",
        ".case\n",
        ".CASE ON\n",
        ".case off\n",
    ] {
        assert!(first(source).is_ok(), "{source}");
    }
}

/// Programs measured on ca65 V2.19 that change how names are read, with
/// the bytes it wrote, or `None` if it refused the program.
const READING: &[(&str, Option<&str>)] = &[
    (".case off\nfoo = 1\n.byte FOO, Foo, foo\n", Some("010101")),
    (
        "Foo = 1\n.case off\n.byte .defined(FOO), .defined(Foo)\n",
        Some("0000"),
    ),
    (".case off\nfoo = 1\n.case on\n.byte FOO\n", Some("01")),
    (
        ".case off\nfoo = 1\n.case on\n.byte .defined(foo), .defined(FOO)\n",
        Some("0001"),
    ),
    (".case -\nab = 2\n.byte AB\n", Some("02")),
    (
        ".case off\n.macro m p\n.byte P\n.endmacro\nM 3\n",
        Some("03"),
    ),
    (".macro m\n.byte 1\n.endmacro\n.case off\nm\n", None),
    (".case off\n.define d 4\n.byte D\n", Some("04")),
    (".case off\n.byte .strlen(.string(abc))\n", Some("03")),
    (".case off\n.byte .xmatch(abc, ABC)\n", Some("01")),
    (".case off\n.ident(\"abc\") = 5\n.byte ABC\n", Some("05")),
    (".case off\n.byte \"ab\"\n", Some("6162")),
    (".case off\nlab:\n@x: .byte <@X\n", Some("00")),
    (".macro m\n.byte q\n.endmacro\nq = 6\n.case off\nm\n", None),
    (
        ".case off\n.macro m\n.byte q\n.endmacro\nQ = 7\n.case on\nM\n",
        Some("07"),
    ),
    (".case 0\n", None),
    (
        ".case off\n.if 0\n.case on\n.endif\nfoo = 8\n.byte FOO\n",
        Some("08"),
    ),
    (".localchar '?'\nlab:\n?x: .byte <?x\n", Some("00")),
    (".localchar '?'\nlab:\n@x: .byte 1\n", None),
    (".localchar '#'\n", None),
    (".localchar \"?\"\n", None),
    (".localchar\n", None),
    (".localchar '?', 1\n", None),
    (".localchar '?'\n?:\n.byte <?-\n", Some("00")),
    (
        ".localchar '?'\n.localchar '@'\nlab:\n@x: .byte <@x\n",
        Some("00"),
    ),
    (".localchar '?'\n.ident(\"?x\") = 9\n.byte ?x\n", Some("09")),
    (".localchar '?'\n.ident(\"@x\") = 9\n", None),
    (
        ".macro m\n.byte <?x\n.endmacro\n.localchar '?'\nlab:\n?x: m\n",
        None,
    ),
    // A body keeps the local character it was read with.
    (
        ".localchar '?'\n.macro m\n.byte .defined(?x)\n.endmacro\n.localchar '@'\nlab:\n@x: m\n",
        Some("00"),
    ),
    (
        ".localchar '?'\nlab:\n?x:\n.macro m\n.byte .defined(?x)\n.endmacro\n.localchar '@'\nm\n",
        Some("01"),
    ),
    (".localchar '?'\nlab:\n?x:\n@x: .byte 1\n", None),
];

#[test]
fn reading_settings_match_ca65() {
    for &(source, hex) in READING {
        let got = assemble(source);
        assert_eq!(got.as_deref().ok(), hex, "{source:?}: {got:?}");
    }
}

/// Programs measured on ca65 V2.19 that turn on `.feature`s changing how
/// names and numbers are read, with the bytes it wrote, or `None` if it
/// refused the program. None uses an address, which the harness places
/// differently from ld65.
const FEATURES: &[(&str, Option<&str>)] = &[
    (
        ".feature at_in_identifiers\nfoo@bar = 1\n.byte foo@bar\n",
        Some("01"),
    ),
    (
        ".feature at_in_identifiers\nfoo@ = 7\n.byte foo@\n",
        Some("07"),
    ),
    (
        ".feature at_in_identifiers\nlab:\n@x@y = 2\n.byte @x@y\n",
        Some("02"),
    ),
    ("foo@bar = 1\n", None),
    // t.s:1: Error: Expected ‘:’ after identifier to form a label but found ‘@bar’
    (".feature at_in_identifiers -\nfoo@bar = 1\n", None),
    // t.s:2: Error: Expected ‘:’ after identifier to form a label but found ‘@bar’
    (".feature at_in_identifiers off\nfoo@bar = 1\n", None),
    // t.s:2: Error: Expected ‘:’ after identifier to form a label but found ‘@bar’
    (
        ".feature at_in_identifiers on\nfoo@bar = 1\n.byte foo@bar\n",
        Some("01"),
    ),
    (
        ".feature AT_IN_IDENTIFIERS +\nfoo@bar = 1\n.byte foo@bar\n",
        Some("01"),
    ),
    (
        ".feature dollar_in_identifiers\nfoo$ = 3\n.byte foo$\n",
        Some("03"),
    ),
    (
        ".feature dollar_in_identifiers\nfoo$1 = 3\n.byte foo$1, $10\n",
        Some("0310"),
    ),
    (
        ".feature dollar_in_identifiers\nlab:\n@x$ = 2\n.byte @x$\n",
        Some("02"),
    ),
    ("foo$ = 3\n", None),
    // t.s:1: Error: Hexadecimal digit expected
    (
        ".feature leading_dot_in_identifiers\n.foo = 4\n.byte .foo\n",
        Some("04"),
    ),
    (
        ".feature leading_dot_in_identifiers\n.FOO = 4\n.byte .foo\n",
        Some("04"),
    ),
    (
        ".feature leading_dot_in_identifiers\n.macro .mm\n.byte 5\n.endmacro\n.mm\n",
        Some("05"),
    ),
    (
        ".feature leading_dot_in_identifiers\n.byte .lobyte($1234), 6\n",
        Some("3406"),
    ),
    (
        ".feature leading_dot_in_identifiers\n.ifdef .foo\n.byte 1\n.endif\n.byte 2\n",
        Some("02"),
    ),
    (
        ".feature leading_dot_in_identifiers\n.byte .defined(.bogus)\n",
        Some("00"),
    ),
    (".foo = 4\n", None),
    // t.s:1: Error: ‘.FOO’ is not a recognized control command
    (
        ".feature underline_in_numbers\n.dword %1100_0011, $12_34, 1_000, 0_1, 1_2h, $1__2, 4_294_967_295\n",
        Some("c300000034120000e8030000010000001200000012000000ffffffff"),
    ),
    (".feature underline_in_numbers\n.byte 12_h\n", None),
    // t.s:2: Error: Number may not end with underline
    (".feature underline_in_numbers\n.byte $1_\n", None),
    // t.s:2: Error: Number may not end with underline
    (".feature underline_in_numbers\n.byte 0_\n", None),
    // t.s:2: Error: Number may not end with underline
    (".feature underline_in_numbers\n.byte $_1\n", None),
    // t.s:2: Error: Hexadecimal digit expected
    (".feature underline_in_numbers\n.dword $ffff_ffff_f\n", None),
    // t.s:2: Error: Overflow in hexadecimal number
    (".byte 1_0\n", None),
    // t.s:1: Error: Expected ‘end-of-line’ but found ‘_0’
    (
        ".feature dollar_is_pc\n.byte $ - *, <($+2-*)\n",
        Some("0002"),
    ),
    (".feature dollar_is_pc\n.word $10, $a\n", Some("10000a00")),
    (
        ".feature dollar_is_pc\n.byte .match($, *), .match($, $), .xmatch($, *)\n",
        Some("000100"),
    ),
    (".feature dollar_is_pc\n.word $$10\n", None),
    // t.s:2: Error: Expected ‘end-of-line’ but found ‘16’
    (".feature dollar_is_pc\n.word 2 $ 3\n", None),
    // t.s:2: Error: Expected ‘end-of-line’ but found ‘$’
    (".feature dollar_is_pc\n$ = 5\n", None),
    // t.s:2: Error: Expected ‘end-of-line’ but found ‘$’
    (".byte $ - *\n", None),
    // t.s:1: Error: Hexadecimal digit expected
    (
        ".feature dollar_is_pc, dollar_in_identifiers\nx$ = 1\n.byte x$\n",
        Some("01"),
    ),
    (
        ".feature dollar_is_pc, underline_in_numbers\n.byte $_1\n",
        None,
    ),
    // t.s:2: Error: Expected ‘end-of-line’ but found ‘_1’
    (
        ".feature at_in_identifiers\n.macro m\nf@g = 9\n.endmacro\n.feature at_in_identifiers -\nm\n.byte f@g\n",
        None,
    ),
    // t.s:7: Error: Expected ‘end-of-line’ but found ‘@g’
    (
        ".macro m\n.byte foo@bar\n.endmacro\n.feature at_in_identifiers\nfoo@bar = 1\nm\n",
        None,
    ),
    // t.s:6: Error: Expected ‘end-of-line’ but found ‘@bar’
    (".feature bogus\n", None),
    // t.s:1: Error: Invalid feature: ‘bogus’
    (".feature\n", None),
    // t.s:1: Error: Expected an identifier but found ‘end-of-line’
    (".feature at_in_identifiers +,\n", None),
    // t.s:1: Error: Expected an identifier but found ‘end-of-line’
    (".feature at_in_identifiers 1\n", None),
    // t.s:1: Error: Expected ON or OFF but found ‘1’
    (".feature at_in_identifiers + x\n", None),
    // t.s:1: Error: Expected ‘end-of-line’ but found ‘X’
    (
        ".if 0\n.feature at_in_identifiers\n.endif\nfoo@bar = 1\n",
        None,
    ),
    // t.s:4: Error: Expected ‘:’ after identifier to form a label but found ‘@bar’
];

#[test]
fn token_features_match_ca65() {
    for &(source, hex) in FEATURES {
        let got = assemble(source);
        assert_eq!(got.as_deref().ok(), hex, "{source:?}: {got:?}");
    }
}

#[test]
fn feature_says_what_is_wrong() {
    for (source, message) in [
        (".feature bogus\n", "Invalid feature: `bogus`"),
        (".feature\n", "Expected an identifier"),
        (".feature dollar_is_pc,\n", "Expected an identifier"),
        (".feature dollar_is_pc 1\n", "Expected ON or OFF"),
        (".feature dollar_is_pc + x\n", "Expected end-of-line"),
        (
            ".feature c_comments\n",
            "the `c_comments` feature is not lowered yet",
        ),
        (
            ".feature addrsize\n",
            "the deprecated `addrsize` feature is not lowered yet",
        ),
    ] {
        let problem = first(source).err().map(|d| d.message().to_owned());
        assert_eq!(problem.as_deref(), Some(message), "{source}");
    }
}

#[test]
fn localchar_says_what_is_wrong() {
    for (source, message) in [
        (".localchar '#'\n", "Invalid start character for locals"),
        (".localchar \"?\"\n", "Expected a character constant"),
        (".localchar\n", "Expected a character constant"),
        (".localchar '?', 1\n", "Expected end-of-line"),
    ] {
        let problem = first(source).err().map(|d| d.message().to_owned());
        assert_eq!(problem.as_deref(), Some(message), "{source}");
    }
}

/// `source` assembled with every label in bank 3, and the values `.bank`
/// was asked about.
fn banked(source: &str) -> Result<(String, Vec<Ir>), String> {
    let (_, first) = pass_banked(source, HashMap::new(), Some(3))?;
    let (bytes, mini) = pass_banked(source, first.finals, Some(3))?;
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok((hex, mini.banked.into_inner()))
}

#[test]
fn bank_asks_the_assembler() {
    // The assembler answers with a name it fills in at the link.
    let (hex, asked) = banked(".byte <.bank(h)\n.dword .bank(h+1)\nh: nop\n").expect("assembles");
    assert_eq!(hex, "0303000000ea");
    assert_eq!(
        asked,
        [
            Ir::Sym("h".to_owned()),
            Ir::Bin(
                asm198x_ir::BinOp::Add,
                Box::new(Ir::Sym("h".to_owned())),
                Box::new(Ir::Num(1))
            )
        ]
    );
    // It is never a constant, so `.res` refuses it, as ca65 does.
    assert!(banked(".res .bank(h)\nh: nop\n").is_err());
    // One value, as ca65's parser requires.
    assert!(banked(".byte <.bank()\n").is_err());
    assert!(banked(".byte <.bank(h, h)\nh: nop\n").is_err());
    // An assembler that does not link leaves it not lowered.
    let refused = assemble(".byte <.bank(h)\nh: nop\n").expect_err("not lowered");
    assert!(refused.contains("`.bank`"), "{refused}");
}
