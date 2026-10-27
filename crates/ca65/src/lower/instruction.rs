//! Instructions, lowered to the form ca65 chooses.
//!
//! ca65 chooses between a zero-page form and an absolute one by the
//! operand's address size (see [`Size`]), and `z:`, `a:` and `f:` before the
//! operand force one. A form the CPU lacks is ca65's "Illegal addressing
//! mode". Range checks, such as a zero-page operand above `$FF`, are left to
//! the back end, which has the final values.

use asm198x_ir::{Expr as Ir, Operation};
use isa198x::InstructionSet;
use lang198x_syntax::{Diagnostic, SyntaxToken};

use super::expr::{self, invalid};
use super::{Known, Size};
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
    let mnemonic = instruction
        .mnemonic()
        .map(|t| t.text().to_ascii_uppercase())
        .ok_or_else(|| invalid(range, "expected a mnemonic"))?;
    let set = set(Mode::of(profile).cpu());
    let mut operands = instruction.operands();
    let operand = operands.next();
    if let Some(extra) = operands.next() {
        return Err(invalid(
            extra.syntax().text_range(),
            "expected the end of the line",
        ));
    }
    let has = |mode: &str| set.find_form(&mnemonic, mode).is_some();
    let illegal = || {
        invalid(
            range,
            "this addressing mode is not one this instruction has",
        )
    };
    let (mode, value) = match operand {
        None => {
            let mode = ["implied", "accumulator"]
                .into_iter()
                .find(|mode| has(mode))
                .ok_or_else(illegal)?;
            (mode, None)
        }
        Some(operand) => choose(&operand, &has, known).map_err(|e| e.unwrap_or_else(illegal))?,
    };
    let form = set.find_form(&mnemonic, mode).ok_or_else(illegal)?;
    Ok(Operation::Instruction {
        mnemonic,
        mode: form.mode,
        operands: value.into_iter().collect(),
    })
}

/// The instruction set a CPU mode assembles.
fn set(cpu: Cpu) -> &'static InstructionSet {
    match cpu {
        Cpu::Mos6502 => &isa198x::mos6502::SET,
    }
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
    if let Some(immediate) = operand.immediate() {
        return Ok(("immediate", Some(lowered(immediate.expr())?)));
    }
    if operand.long_indirect().is_some() {
        return Err(None);
    }
    if let Some(indirect) = operand.indirect() {
        let value = lowered(indirect.expr())?;
        let inner = register(indirect.inner_register());
        let outer = register(indirect.outer_register());
        let mode = match (inner.as_deref(), outer.as_deref()) {
            (Some("x"), None) => "(indirect,x)",
            (None, Some("y")) => "(indirect),y",
            (None, None) => "indirect",
            _ => return Err(None),
        };
        return Ok((mode, Some(value)));
    }
    let value = lowered(operand.expr())?;
    let forced = operand
        .address_size()
        .map(|t| t.text().trim_end_matches(':').to_ascii_lowercase());
    let size = match forced.as_deref() {
        Some("z") => Size::Zero,
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

fn register(token: Option<SyntaxToken>) -> Option<String> {
    token.map(|t| t.text().to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
