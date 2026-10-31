//! The CPU each line is read for, measured on ca65 V2.19 at `8d86ca7`.
//! ca65 reads a register name as a register only under a CPU that has it,
//! and it reads a macro's or a repeat's lines once, when it collects them.

use crate::ast::Statement;
use crate::expand::Expander;
use crate::expand::tests::host::TestHost;
use crate::{Cpu, Profile};

/// The CPU of each line `source` hands over, and how many problems the
/// expander found.
fn cpus(source: &str) -> (Vec<Cpu>, usize) {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(&[]);
    let mut cpus = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        cpus.push(line.cpu());
    }
    (cpus, expander.diagnostics().len())
}

/// For each instruction `source` hands over, whether its `z` was read as
/// a register: then `(zp),z` is one operand, and otherwise it is two,
/// which lowering refuses as ca65 does.
fn registers(source: &str) -> Vec<bool> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(&[]);
    let mut found = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        if let Some(Statement::Instruction(instruction)) = line.line().statement() {
            found.push(instruction.operands().count() == 1);
        }
    }
    found
}

const SIX: Cpu = Cpu::Mos6502;
const CSG: Cpu = Cpu::Csg4510;

#[test]
fn the_cpu_commands_choose_the_cpu_of_the_lines_after_them() {
    // A command's own line is read under the CPU it chooses.
    let (got, problems) = cpus(" nop\n.p4510\n nop\n.p02\n nop\n");
    assert_eq!(got, [SIX, CSG, CSG, SIX, SIX]);
    assert_eq!(problems, 0);
    let (got, _) = cpus(".setcpu \"4510\"\n nop\n.setcpu \"6502\"\n nop\n");
    assert_eq!(got, [CSG, CSG, SIX, SIX]);
    let source = ".p4510\n.pushcpu\n.p02\n nop\n.popcpu\n nop\n";
    let (got, _) = cpus(source);
    assert_eq!(got, [CSG, CSG, SIX, SIX, CSG, CSG]);
    // A CPU this crate does not model, and a `.popcpu` with nothing to
    // pop, are left for lowering, which refuses them.
    let (got, _) = cpus(".setcpu \"nope\"\n nop\n.popcpu\n nop\n");
    assert_eq!(got, [SIX, SIX, SIX, SIX]);
}

#[test]
fn ifp4510_asks_about_the_cpu() {
    // Measured: `01 03`.
    let source = ".setcpu \"4510\"\n.ifp4510\n.byte 1\n.endif\n.pushcpu\n.p02\n\
                  .ifp4510\n.byte 2\n.endif\n.popcpu\n.ifp4510\n.byte 3\n.endif\n";
    assert_eq!(super::run(source), Ok(vec![1, 3]));
}

#[test]
fn a_line_is_read_under_the_cpu_it_is_reached_under() {
    assert_eq!(registers(".p4510\n lda ($12),z\n"), [true]);
    assert_eq!(registers(" lda ($12),z\n"), [false]);
    assert_eq!(
        registers(".p4510\n lda ($12),z\n.p02\n lda ($12),z\n"),
        [true, false]
    );
}

#[test]
fn a_repeat_or_macro_keeps_the_cpu_it_was_collected_under() {
    // Measured: an error on each pass, as ca65 read the body under the
    // 6502, and none when the body was read under the 4510.
    let source = ".repeat 2\n lda ($12),z\n.p4510\n.endrepeat\n";
    assert_eq!(registers(source), [false, false]);
    let source = ".p4510\n.repeat 2\n lda ($12),z\n.p02\n.endrepeat\n";
    assert_eq!(registers(source), [true, true]);
    // Measured: an error, as the macro was read under the 6502.
    let source = ".macro m\n lda ($12),z\n.endmacro\n.p4510\nm\n";
    assert_eq!(registers(source), [false]);
    let source = ".p4510\n.macro m\n lda ($12),z\n.endmacro\n.p02\nm\n";
    assert_eq!(registers(source), [true]);
}

#[test]
fn each_cpu_test_asks_about_its_own_cpu() {
    // Measured: under each CPU's command, only that CPU's `.IFP*` holds.
    for (index, cpu) in Cpu::ALL.into_iter().enumerate() {
        let tests: String = Cpu::ALL
            .into_iter()
            .enumerate()
            .map(|(n, other)| format!(".if{}\n.byte {n}\n.endif\n", &other.command()[1..]))
            .collect();
        let source = format!("{}\n{tests}", cpu.command().to_ascii_uppercase());
        assert_eq!(
            super::run(&source),
            Ok(vec![u8::try_from(index).expect("small")]),
            "{}",
            cpu.name()
        );
    }
}

#[test]
fn setcpu_names_every_cpu_in_any_case() {
    for cpu in Cpu::ALL {
        let source = format!(".setcpu \"{}\"\n nop\n", cpu.name().to_ascii_uppercase());
        let (got, _) = cpus(&source);
        assert_eq!(got.last(), Some(&cpu), "{}", cpu.name());
    }
    // A name ca65 does not know leaves the CPU as it was.
    let (got, _) = cpus(".setcpu \"6510\"\n nop\n");
    assert_eq!(got, [SIX, SIX]);
}
