//! A tiny assembler, just enough to compare the expander's lines with ca65's
//! bytes: labels, `=`, `.byte`, `.word`, `nop`, `rts` and `lda #value`.

use std::collections::{HashMap, HashSet};

use crate::Profile;
use crate::ast::{Expr, NameRef, Statement, View};
use crate::expand::{ActiveLine, Expander, Host};

#[derive(Default)]
pub(super) struct TestHost {
    symbols: HashMap<String, i64>,
    referenced: HashSet<String>,
    pc: i64,
    pub(super) bytes: Vec<u8>,
    pub(super) problems: Vec<String>,
}

impl TestHost {
    fn value(&mut self, expr: &Expr) -> Result<i64, String> {
        let truth = |b: bool| i64::from(b);
        Ok(match expr {
            Expr::Literal(literal) => {
                let text = literal
                    .token()
                    .map(|t| t.text().to_owned())
                    .unwrap_or_default();
                if text == "*" {
                    self.pc
                } else if let Some(hex) = text.strip_prefix('$') {
                    i64::from_str_radix(hex, 16).map_err(|e| e.to_string())?
                } else {
                    text.parse()
                        .map_err(|_| format!("`{text}` is not a number here"))?
                }
            }
            Expr::Name(name) => {
                let text = name.text();
                self.referenced.insert(text.clone());
                *self
                    .symbols
                    .get(&text)
                    .ok_or_else(|| format!("`{text}` is not defined"))?
            }
            Expr::Paren(paren) => self.value(&paren.expr().ok_or("empty")?)?,
            Expr::Unary(unary) => {
                let operand = self.value(&unary.operand().ok_or("no operand")?)?;
                match unary.op().map(|t| t.text().to_owned()).as_deref() {
                    Some("+") => operand,
                    Some("-") => -operand,
                    Some("!") => truth(operand == 0),
                    Some("<") => operand & 0xFF,
                    Some(">") => (operand >> 8) & 0xFF,
                    other => return Err(format!("no prefix {other:?} here")),
                }
            }
            Expr::Binary(binary) => {
                let lhs = self.value(&binary.lhs().ok_or("no left")?)?;
                let rhs = self.value(&binary.rhs().ok_or("no right")?)?;
                match binary.op().map(|t| t.text().to_owned()).as_deref() {
                    Some("+") => lhs + rhs,
                    Some("-") => lhs - rhs,
                    Some("*") => lhs * rhs,
                    Some("=") => truth(lhs == rhs),
                    Some("<>") => truth(lhs != rhs),
                    Some(">") => truth(lhs > rhs),
                    Some("<") => truth(lhs < rhs),
                    other => return Err(format!("no operator {other:?} here")),
                }
            }
            Expr::Unnamed(_) | Expr::Call(_) => return Err("not in this test host".into()),
        })
    }

    /// Assembles one line the expander hands over.
    fn assemble(&mut self, active: &ActiveLine) {
        let line = active.line();
        if let Some(name) = line.label().and_then(|l| l.name()) {
            // ca65 refuses a label defined twice.
            if self.symbols.insert(name.text(), self.pc).is_some() {
                self.problems
                    .push(format!("`{}` is defined twice", name.text()));
            }
        }
        if active.label_only() {
            return;
        }
        let result = match line.statement() {
            None => Ok(()),
            Some(Statement::Assignment(assignment)) => assignment
                .value()
                .ok_or_else(|| "no value".to_owned())
                .and_then(|value| self.value(&value))
                .map(|value| {
                    let name = assignment.name().map(|n| n.text()).unwrap_or_default();
                    self.symbols.insert(name, value);
                }),
            Some(Statement::Command(command)) => {
                let word = command.word().map(|w| w.text().to_ascii_lowercase());
                let width = match word.as_deref() {
                    Some(".byte") => 1,
                    Some(".word") => 2,
                    other => return self.problems.push(format!("no command {other:?} here")),
                };
                command.values().try_for_each(|value| {
                    let value = self.value(&value)?;
                    self.emit(value, width);
                    Ok(())
                })
            }
            Some(Statement::Instruction(instruction)) => {
                let mnemonic = instruction
                    .mnemonic()
                    .map(|t| t.text().to_ascii_lowercase());
                match mnemonic.as_deref() {
                    Some("nop") => {
                        self.emit(0xEA, 1);
                        Ok(())
                    }
                    Some("rts") => {
                        self.emit(0x60, 1);
                        Ok(())
                    }
                    Some("lda") => instruction
                        .operands()
                        .next()
                        .and_then(|o| o.immediate())
                        .and_then(|i| i.expr())
                        .ok_or_else(|| "only `lda #value` here".to_owned())
                        .and_then(|value| {
                            let value = self.value(&value)?;
                            self.emit(0xA9, 1);
                            self.emit(value, 1);
                            Ok(())
                        }),
                    other => Err(format!("no instruction {other:?} here")),
                }
            }
            Some(other) => Err(format!("no statement {other:?} here")),
        };
        if let Err(problem) = result {
            self.problems.push(problem);
        }
    }

    fn emit(&mut self, value: i64, width: usize) {
        for i in 0..width {
            // Truncation is the point: each byte is the next eight bits.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            self.bytes.push((value >> (8 * i)) as u8);
            self.pc += 1;
        }
    }
}

impl Host for TestHost {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        self.value(expr)
    }

    fn is_defined(&mut self, name: &NameRef) -> bool {
        self.symbols.contains_key(&name.text())
    }

    fn is_referenced(&mut self, name: &NameRef) -> bool {
        self.referenced.contains(&name.text())
    }

    fn is_constant(&mut self, expr: &Expr) -> bool {
        // The address is not a constant until linking, as in ca65.
        let uses_address = expr.syntax().text().contains('*');
        !uses_address && self.value(expr).is_ok()
    }
}

/// What a program gives: its bytes, or every problem the expander and the
/// host found.
pub(super) fn run(source: &str) -> Result<Vec<u8>, Vec<String>> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("short text parses");
    let mut host = TestHost::default();
    while let Some(line) = expander.next_line(&mut host) {
        host.assemble(&line);
    }
    let mut problems: Vec<String> = expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message().to_owned())
        .collect();
    problems.extend(host.problems);
    if problems.is_empty() {
        Ok(host.bytes)
    } else {
        Err(problems)
    }
}
