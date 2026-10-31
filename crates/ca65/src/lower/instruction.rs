//! Instructions, lowered to the form ca65 chooses.
//!
//! ca65 chooses between a zero-page form and an absolute one by the
//! operand's address size (see [`Size`]), and `z:`, `a:` and `f:` before the
//! operand force one. A form the CPU lacks is ca65's "Illegal addressing
//! mode". Range checks, such as a zero-page operand above `$FF`, are left to
//! the back end, which has the final values.

use asm198x_ir::{BinOp, Expr as Ir, Operation};
use isa198x::InstructionSet;
use lang198x_syntax::{Diagnostic, SyntaxToken, TextRange};

use super::expr::{self, invalid};
use super::{Known, Size, Value};
use crate::ast::{Instruction, Operand, View};
use crate::{Cpu, Mode, Profile};

/// Lowers `instruction` as ca65 V2.19 assembles it under `profile`.
///
/// ```
/// use asm198x_ir::{Expr, Operation};
/// use lang198x_ca65::ast::{SourceFile, Statement, View};
/// use lang198x_ca65::lower::{self, Known, Value};
/// use lang198x_ca65::{Profile, parse};
///
/// struct Nothing;
///
/// impl Known for Nothing {
///     fn name(&self, _: &str) -> Value {
///         Value::Unknown
///     }
///     fn pc(&self) -> Value {
///         Value::Unknown
///     }
/// }
///
/// let parse = parse("lda later,x\n", Profile::CA65_2_19_6502)?;
/// let line = SourceFile::cast(parse.tree().root()).unwrap().lines().next().unwrap();
/// let Some(Statement::Instruction(instruction)) = line.statement() else { panic!() };
/// let op = lower::instruction(&instruction, Profile::CA65_2_19_6502, &Nothing).expect("lowers");
/// let Operation::Instruction { mnemonic, mode, operands } = op else { panic!() };
/// assert_eq!((mnemonic.as_str(), mode), ("LDA", "absolute,x"));
/// assert_eq!(operands, [Expr::Sym("later".to_owned())]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
///
/// A diagnostic, with its range in the instruction's tree, if ca65 refuses
/// the instruction.
pub fn instruction(
    instruction: &Instruction,
    profile: Profile,
    known: &dyn Known,
) -> Result<Operation, Diagnostic> {
    let range = instruction.syntax().text_range();
    let written = instruction
        .mnemonic()
        .map(|t| t.text().to_ascii_uppercase())
        .ok_or_else(|| invalid(range, "expected a mnemonic"))?;
    let cpu = Mode::of(profile).cpu();
    if !cpu.lowered() {
        return Err(super::expr::unsupported(
            range,
            &format!("an instruction for the `{}`", cpu.name()),
        ));
    }
    // ca65's tables spell some instructions twice: `INA` and `DEA` are
    // `INC` and `DEC` of the accumulator, and on the 65CE02 family `PHD` is
    // `PHW`.
    let mnemonic = match (cpu, written.as_str()) {
        (Cpu::Csg65ce02 | Cpu::Csg4510, "PHD") => "PHW".to_owned(),
        // ca65's 65CE02 table spells `NOP` as `EOM` too.
        (Cpu::Csg65ce02, "EOM") => "NOP".to_owned(),
        // ca65 and Isa198x name three of the 6502's undocumented
        // instructions differently.
        (Cpu::Nmos6502X, "ALR") => "ASR".to_owned(),
        (Cpu::Nmos6502X, "AXS") => "SBX".to_owned(),
        (Cpu::Nmos6502X, "LAX") if operands_immediate(instruction) => "LXA".to_owned(),
        (_, "INA") if cpu.is_mnemonic("INA") => "INC".to_owned(),
        (_, "DEA") if cpu.is_mnemonic("DEA") => "DEC".to_owned(),
        _ => written.clone(),
    };
    let find = |mode: &str| form(cpu, &mnemonic, mode);
    let has = |mode: &str| find(mode).is_some();
    let illegal = || {
        invalid(
            range,
            "this addressing mode is not one this instruction has",
        )
    };
    let operands: Vec<Operand> = instruction.operands().collect();
    if has("zeropage,relative") {
        return bit_branch(&mnemonic, &operands, range, known);
    }
    if has("block") {
        return block_transfer(&mnemonic, &operands, range, known);
    }
    if has("immediate,zeropage") {
        return test(&mnemonic, &operands, range, known);
    }
    // The HuC6280's `TAMn` and `TMAn` are `TAM` and `TMA` with bit `n` set.
    if cpu == Cpu::HuC6280
        && let Some((name, bit)) = mapping_register(&mnemonic)
    {
        if let Some(extra) = operands.first() {
            return Err(invalid(
                extra.syntax().text_range(),
                "expected the end of the line",
            ));
        }
        return Ok(Operation::Instruction {
            mnemonic: name.to_owned(),
            mode: "immediate",
            operands: vec![Ir::Num(1 << bit)],
        });
    }
    let operand = match operands.as_slice() {
        [] => None,
        [operand] => Some(operand),
        // `(address), name` on an instruction that is not a bit branch is
        // an indirect address with a bad register.
        [first, extra, ..] => {
            let message = if first.indirect().is_some() {
                "expected `y`"
            } else {
                "expected the end of the line"
            };
            return Err(invalid(extra.syntax().text_range(), message));
        }
    };
    let (mode, value) = match operand {
        None if mnemonic != written && matches!(written.as_str(), "INA" | "DEA") => {
            ("accumulator", None)
        }
        Some(_) if mnemonic != written && matches!(written.as_str(), "INA" | "DEA") => {
            return Err(illegal());
        }
        None => {
            let mode = ["implied", "accumulator"]
                .into_iter()
                .find(|mode| has(mode))
                .ok_or_else(illegal)?;
            (mode, None)
        }
        Some(operand) => choose(operand, &has, known).map_err(|e| e.unwrap_or_else(illegal))?,
    };
    // `PHW #value` pushes a word.
    let mode = if mode == "immediate" && has("immediate16") {
        "immediate16"
    } else {
        mode
    };
    let form = find(mode).ok_or_else(illegal)?;
    // ca65 lets `TMA` read one mapping register at a time, so its value
    // must have one bit set, or none. A value not known yet is left alone.
    if cpu == Cpu::HuC6280
        && mnemonic == "TMA"
        && let Some(value) = value.as_ref().and_then(|v| expr::constant(v, known))
        && value & value.wrapping_sub(1) != 0
    {
        return Err(invalid(range, "Argument of TMA must be a power of two"));
    }
    // A 65CE02 long branch counts from its opcode plus two, where the
    // shared encoder counts from the end of the instruction, as the
    // 65816's do; the target carries the difference. ACME does the same.
    let value = match (form.mode, value) {
        ("relative16", Some(target)) => {
            Some(Ir::Bin(BinOp::Add, Box::new(target), Box::new(Ir::Num(1))))
        }
        (_, value) => value,
    };
    Ok(Operation::Instruction {
        mnemonic,
        mode: form.mode,
        operands: value.into_iter().collect(),
    })
}

/// Whether `instruction`'s operand is an immediate value.
fn operands_immediate(instruction: &Instruction) -> bool {
    instruction
        .operands()
        .next()
        .is_some_and(|operand| operand.immediate().is_some())
}

/// The instruction sets `cpu` assembles: the 6502's, and the extension
/// over it the CPU adds, if any. A lowered instruction's mode names a form
/// in one of them, the extension's first.
#[must_use]
pub fn sets(cpu: Cpu) -> (&'static InstructionSet, Option<&'static InstructionSet>) {
    let extension = match cpu {
        Cpu::Nmos6502X => Some(&isa198x::nmos6502_undocumented::SET),
        Cpu::Dtv6502 => Some(&isa198x::c64dtv2::SET),
        Cpu::Mos65sc02 => Some(&isa198x::mos65c02::SET),
        Cpu::Mos65c02 => Some(&isa198x::mos65c02::ROCKWELL_SET),
        Cpu::Wdc65c02 => Some(&isa198x::mos65c02::WDC_SET),
        Cpu::Csg65ce02 => Some(&isa198x::csg65ce02::SET),
        Cpu::Csg4510 => Some(&isa198x::csg65ce02::CSG4502_SET),
        Cpu::HuC6280 => Some(&isa198x::huc6280::SET),
        _ => None,
    };
    (&isa198x::mos6502::SET, extension)
}

/// `mnemonic`'s form in `mode` on `cpu`: the extension's, if it has one, or
/// the 6502's.
fn form(cpu: Cpu, mnemonic: &str, mode: &str) -> Option<&'static isa198x::Form> {
    let (base, extension) = sets(cpu);
    extension
        .and_then(|set| set.find_form(mnemonic, mode))
        .or_else(|| base.find_form(mnemonic, mode))
}

/// `TAMn` or `TMAn`: the instruction it spells, and the bit `n`.
fn mapping_register(mnemonic: &str) -> Option<(&'static str, u32)> {
    let (name, bit) = match mnemonic.get(..3)? {
        "TAM" => ("TAM", mnemonic.get(3..)?),
        "TMA" => ("TMA", mnemonic.get(3..)?),
        _ => return None,
    };
    match bit.as_bytes() {
        [digit @ b'0'..=b'7'] => Some((name, u32::from(digit - b'0'))),
        _ => None,
    }
}

/// A HuC6280 block transfer, such as `TII source, destination, length`,
/// as ca65's `PutBlockTransfer` reads it: three values, each a word.
fn block_transfer(
    mnemonic: &str,
    operands: &[Operand],
    range: TextRange,
    known: &dyn Known,
) -> Result<Operation, Diagnostic> {
    let [source, destination, length] = operands else {
        let message = if operands.len() > 3 {
            "expected the end of the line"
        } else {
            "expected `,`"
        };
        let at = operands.get(3).map_or(range, |o| o.syntax().text_range());
        return Err(invalid(at, message));
    };
    Ok(Operation::Instruction {
        mnemonic: mnemonic.to_owned(),
        mode: "block",
        operands: vec![
            plain(source, known)?,
            plain(destination, known)?,
            plain(length, known)?,
        ],
    })
}

/// The HuC6280's `TST #mask, address`, as ca65's `PutTST` reads it: an
/// immediate value, then an address read as any other instruction's is.
fn test(
    mnemonic: &str,
    operands: &[Operand],
    range: TextRange,
    known: &dyn Known,
) -> Result<Operation, Diagnostic> {
    let illegal = |range| {
        invalid(
            range,
            "this addressing mode is not one this instruction has",
        )
    };
    let Some((first, rest)) = operands.split_first() else {
        return Err(illegal(range));
    };
    let mask = first
        .immediate()
        .filter(|_| first.address_size().is_none())
        .and_then(|immediate| immediate.expr())
        .ok_or_else(|| illegal(first.syntax().text_range()))?;
    let mask = expr::expression(&mask, known)?;
    let address = match rest {
        [address] => address,
        [] => return Err(invalid(range, "expected `,`")),
        [_, extra, ..] => {
            return Err(invalid(
                extra.syntax().text_range(),
                "expected the end of the line",
            ));
        }
    };
    let form = |mode: &str| form(Cpu::HuC6280, mnemonic, &format!("immediate,{mode}"));
    let has = |mode: &str| form(mode).is_some();
    let address_range = address.syntax().text_range();
    let (mode, value) =
        choose(address, &has, known).map_err(|e| e.unwrap_or_else(|| illegal(address_range)))?;
    let form = form(mode).ok_or_else(|| illegal(address_range))?;
    Ok(Operation::Instruction {
        mnemonic: mnemonic.to_owned(),
        mode: form.mode,
        operands: [Some(mask), value].into_iter().flatten().collect(),
    })
}

/// A value written as ca65's `Expression` reads it, where `(1)` is a value
/// in parentheses, not an indirect address, and nothing else is allowed.
fn plain(operand: &Operand, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let written = match operand.indirect() {
        Some(indirect)
            if indirect.inner_register().is_none()
                && indirect.outer_register().is_none()
                && operand.address_size().is_none() =>
        {
            indirect.expr()
        }
        _ if operand.address_size().is_none()
            && operand.index().is_none()
            && operand.immediate().is_none() =>
        {
            operand.expr()
        }
        _ => None,
    };
    let written = written.ok_or_else(|| {
        invalid(
            operand.syntax().text_range(),
            "this addressing mode is not one this instruction has",
        )
    })?;
    expr::expression(&written, known)
}

/// A Rockwell bit branch, `BBRn zp, target` or `BBSn zp, target`, as ca65's
/// `PutBitBranch` reads it: two values, each an expression.
fn bit_branch(
    mnemonic: &str,
    operands: &[Operand],
    range: TextRange,
    known: &dyn Known,
) -> Result<Operation, Diagnostic> {
    let [zero, target] = operands else {
        return Err(invalid(range, "expected `,`"));
    };
    let value = |operand: &Operand| plain(operand, known);
    Ok(Operation::Instruction {
        mnemonic: mnemonic.to_owned(),
        mode: "zeropage,relative",
        operands: vec![value(zero)?, value(target)?],
    })
}

/// The mode an operand asks for, and its value. `Err(None)` is a mode the
/// instruction lacks.
fn choose(
    operand: &Operand,
    has: &dyn Fn(&str) -> bool,
    known: &dyn Known,
) -> Result<(&'static str, Option<Ir>), Option<Diagnostic>> {
    let range = operand.syntax().text_range();
    let lowered = |e: Option<crate::ast::Expr>| -> Result<Ir, Option<Diagnostic>> {
        let e = e.ok_or_else(|| invalid(range, "expected a value"))?;
        expr::expression(&e, known).map_err(Some)
    };
    if operand.accumulator().is_some() {
        return Ok(("accumulator", None));
    }
    // ca65 forces a size only on an address: `lda z:#1` and
    // `lda a:($12,x)` are "Illegal addressing mode".
    let forced_on_other = operand.address_size().is_some();
    if let Some(immediate) = operand.immediate() {
        if forced_on_other {
            return Err(None);
        }
        return Ok(("immediate", Some(lowered(immediate.expr())?)));
    }
    if operand.long_indirect().is_some() {
        return Err(None);
    }
    if let Some(indirect) = operand.indirect() {
        if forced_on_other {
            return Err(None);
        }
        let value = lowered(indirect.expr())?;
        let inner = register(indirect.inner_register());
        let outer = register(indirect.outer_register());
        let mode = match (inner.as_deref(), outer.as_deref()) {
            // `JMP (abs,x)` on the CMOS CPUs.
            (Some("x"), None) if !has("(indirect,x)") && has("(absolute,x)") => "(absolute,x)",
            (Some("x"), None) => "(indirect,x)",
            (None, Some("y")) => "(indirect),y",
            (None, Some("z")) => "(indirect),z",
            (Some("s" | "sp"), Some("y")) => "(stack-indirect),y",
            // `JMP (abs)`, or the 65C02's `(zp)`, which Isa198x names
            // `(indirect)`.
            (None, None) if !has("indirect") && has("(indirect)") => "(indirect)",
            (None, None) => "indirect",
            _ => return Err(None),
        };
        return Ok((mode, Some(value)));
    }
    let value = lowered(operand.expr())?;
    let forced = operand
        .address_size()
        .map(|t| t.text().trim_end_matches(':').to_ascii_lowercase());
    // A size forced on a value bigger than it is "Illegal addressing
    // mode", as ca65 takes the value's own size from the forced choices.
    let studied = expr::size(&value, &Defaulted(known));
    let size = match forced.as_deref() {
        Some("z") if studied != Size::Zero => return Err(None),
        Some("z") => Size::Zero,
        Some("a") if studied == Size::Far => return Err(None),
        Some("a") => Size::Absolute,
        Some(_) => Size::Far,
        None => expr::size(&value, known),
    };
    let index = register(operand.index().and_then(|i| i.register()));
    let (zero, absolute) = match index.as_deref() {
        Some("x") => ("zeropage,x", "absolute,x"),
        Some("y") => ("zeropage,y", "absolute,y"),
        Some(_) => return Err(None),
        None if forced.is_none() && has("relative") => return Ok(("relative", Some(value))),
        None if forced.is_none() && has("relative16") => return Ok(("relative16", Some(value))),
        None => ("zeropage", "absolute"),
    };
    let mode = match size {
        Size::Zero if has(zero) => zero,
        // `z:` asks for the zero page, and there is no other choice.
        Size::Zero if forced.is_some() => return Err(None),
        Size::Zero | Size::Absolute if has(absolute) => absolute,
        _ => return Err(None),
    };
    Ok((mode, Some(value)))
}

/// What `known` knows, but with a name not known yet taking no size, as in
/// ca65, where it takes the default and so leaves a forced size alone.
struct Defaulted<'k>(&'k dyn Known);

impl Known for Defaulted<'_> {
    fn name(&self, name: &str) -> Value {
        match self.0.name(name) {
            Value::Unknown => Value::Opaque(Size::Zero),
            other => other,
        }
    }

    fn pc(&self) -> Value {
        self.0.pc()
    }
}

fn register(token: Option<SyntaxToken>) -> Option<String> {
    token.map(|t| t.text().to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
