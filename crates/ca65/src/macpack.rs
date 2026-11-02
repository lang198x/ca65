//! The macro packages `.macpack` reads in, written for this crate.
//!
//! ca65 V2.19 reads a package, such as `generic`, from a `.mac` file that
//! ships with cc65. This crate does not copy those files. Each package here
//! is written from ca65's manual and from what ca65 V2.19 was measured to
//! make, line by line, with every macro, argument shape and error checked.
//! Asm198x records those measurements in its verdict corpus, and the
//! assembler gives the same bytes and refusals.
//!
//! Two things are worth knowing:
//!
//! - ca65 V2.19 defines the `CPU_` constants itself, so the `cpu` package
//!   only warns that it is no longer needed. See [`predefined`].
//! - `scrcode` in the `cbm` package works out an expression's screen code
//!   from the value of the whole expression. ca65's own package gets some
//!   expressions wrong, such as `1+1`; Asm198x names that difference.

/// The text of the package `.macpack` names, in lower case as ca65 spells
/// it, or `None` if there is no such package.
///
/// ```
/// let text = lang198x_ca65::macpack::package("generic").expect("a package");
/// assert!(text.contains(".macro add"));
/// assert!(lang198x_ca65::macpack::package("GENERIC").is_none());
/// ```
#[must_use]
pub fn package(name: &str) -> Option<String> {
    Some(match name {
        "generic" => GENERIC.to_owned(),
        "longbranch" => longbranch(),
        "cpu" => CPU.to_owned(),
        "cbm" => scrcode(CBM, true),
        "apple2" => scrcode(APPLE2, false),
        "atari" => scrcode(ATARI, true),
        "module" => MODULE.to_owned(),
        _ => return None,
    })
}

/// The value of a constant ca65 V2.19 defines before the program starts,
/// such as `CPU_6502`, or `None` if it defines no such name. A program
/// cannot define one of these names itself.
///
/// ```
/// assert_eq!(lang198x_ca65::macpack::predefined("CPU_ISET_65816"), Some(0x40));
/// assert_eq!(lang198x_ca65::macpack::predefined("cpu_6502"), None);
/// ```
#[must_use]
pub fn predefined(name: &str) -> Option<i64> {
    PREDEFINED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

/// The `CPU_` constants, measured on ca65 V2.19: one bit for each
/// instruction set, and for each CPU the bits of the sets it has, as `.cpu`
/// gives them.
const PREDEFINED: &[(&str, i64)] = &[
    ("CPU_ISET_NONE", 0x0001),
    ("CPU_ISET_6502", 0x0002),
    ("CPU_ISET_6502X", 0x0004),
    ("CPU_ISET_6502DTV", 0x0008),
    ("CPU_ISET_65SC02", 0x0010),
    ("CPU_ISET_65C02", 0x0020),
    ("CPU_ISET_65816", 0x0040),
    ("CPU_ISET_SWEET16", 0x0080),
    ("CPU_ISET_HUC6280", 0x0100),
    ("CPU_ISET_M740", 0x0200),
    ("CPU_ISET_4510", 0x0400),
    ("CPU_ISET_45GS02", 0x0800),
    ("CPU_ISET_W65C02", 0x1000),
    ("CPU_ISET_65CE02", 0x2000),
    ("CPU_NONE", 0x0001),
    ("CPU_6502", 0x0002),
    ("CPU_6502X", 0x0006),
    ("CPU_6502DTV", 0x000A),
    ("CPU_65SC02", 0x0012),
    ("CPU_65C02", 0x0032),
    ("CPU_65816", 0x0052),
    ("CPU_SWEET16", 0x0080),
    ("CPU_HUC6280", 0x0132),
    ("CPU_M740", 0x0202),
    ("CPU_4510", 0x2422),
    ("CPU_45GS02", 0x2C22),
    ("CPU_W65C02", 0x1032),
    ("CPU_65CE02", 0x2022),
];

/// `generic`: short names for an add or subtract and for the branches after
/// a compare. `add` and `sub` take an operand that may hold a comma, as in
/// `add ($10),y`, so they take it in two parts.
const GENERIC: &str = "\
.macro add a1, a2
        clc
.ifblank a2
        adc a1
.else
        adc a1, a2
.endif
.endmacro
.macro sub a1, a2
        sec
.ifblank a2
        sbc a1
.else
        sbc a1, a2
.endif
.endmacro
.macro bge target
        bcs target
.endmacro
.macro blt target
        bcc target
.endmacro
.macro bgt target
.local over
        beq over
        bcs target
over:
.endmacro
.macro ble target
        beq target
        bcc target
.endmacro
.macro bnz target
        bne target
.endmacro
.macro bze target
        beq target
.endmacro
";

/// The long branches of `longbranch`: each name, its branch, and the branch
/// on the opposite condition.
const LONG_BRANCHES: [(&str, &str, &str); 8] = [
    ("jeq", "beq", "bne"),
    ("jne", "bne", "beq"),
    ("jmi", "bmi", "bpl"),
    ("jpl", "bpl", "bmi"),
    ("jcs", "bcs", "bcc"),
    ("jcc", "bcc", "bcs"),
    ("jvs", "bvs", "bvc"),
    ("jvc", "bvc", "bvs"),
];

/// `longbranch`: a branch that reaches anywhere. It is the short branch when
/// the target is a name defined above that it can reach, and otherwise the
/// opposite branch over a `jmp`. A number on its own takes the `jmp`; any
/// other target must be a name, as ca65's `.def` requires.
fn longbranch() -> String {
    LONG_BRANCHES
        .iter()
        .map(|(name, branch, opposite)| {
            format!(
                ".macro {name} target\n\
                 .if .tcount({{target}}) = 1 .and .match({{target}}, 0)\n\
                 \x20       {opposite} *+5\n\
                 \x20       jmp target\n\
                 .elseif .def(target) .and .const((*+2)-(target)) .and ((*+2)-(target) <= 127)\n\
                 \x20       {branch} target\n\
                 .else\n\
                 \x20       {opposite} *+5\n\
                 \x20       jmp target\n\
                 .endif\n\
                 .endmacro\n"
            )
        })
        .collect()
}

/// `cpu`: ca65 V2.19 defines the `CPU_` constants itself.
const CPU: &str = ".warning \"'.macpack cpu' is no longer required\"\n";

/// `apple2`'s screen code of a character `v`: the character with its top
/// bit set, which is out of range for a character that already has it.
const APPLE2: &str = "(v)+$80";

/// The C64's screen code of a PETSCII character `v`.
const CBM: &str = "<((v) + $80*((v) < $20) - $40*((v) >= $40 .and (v) < $60) \
                   - $20*((v) >= $60 .and (v) < $80) + $40*((v) >= $80 .and (v) < $A0) \
                   - $40*((v) >= $A0 .and (v) < $C0) + $80*((v) >= $C0))";

/// The Atari's internal code of an ATASCII character `v`.
const ATARI: &str = "<((v) + $40*((v) < $20) - $20*((v) >= $20 .and (v) < $60) \
                     + $40*((v) >= $80 .and (v) < $A0) - $20*((v) >= $A0 .and (v) < $E0))";

/// A package defining `scrcode`, which lays down each of up to nine
/// arguments as screen codes: a string a character at a time, after the
/// character map, or a number, a character or a name, which must be 255 at
/// most. `code` is the screen code of a character `v`. With `unsigned`, a
/// value below 0 is refused too, as the `cbm` and `atari` packages do; the
/// `apple2` package lays one down plus `$80`.
fn scrcode(code: &str, unsigned: bool) -> String {
    let of = |value: &str| code.replace('v', value);
    let range = if unsigned {
        "(a1) < 0 .or (a1) > 255"
    } else {
        "(a1) > 255"
    };
    format!(
        ".macro scrcode a1, a2, a3, a4, a5, a6, a7, a8, a9\n\
         .ifblank a1\n\
         .exitmacro\n\
         .endif\n\
         .if .match(.left(1, {{a1}}), \"\")\n\
         .if .tcount({{a1}}) <> 1\n\
         .error \"scrcode: invalid argument type\"\n\
         .else\n\
         .repeat .strlen(a1), i\n\
         .byte {string}\n\
         .endrepeat\n\
         .endif\n\
         .elseif .match(.left(1, {{a1}}), 0) .or .match(.left(1, {{a1}}), 'a') .or .match(.left(1, {{a1}}), name)\n\
         .if {range}\n\
         .error \"scrcode: Character constant out of range\"\n\
         .else\n\
         .byte {value}\n\
         .endif\n\
         .else\n\
         .error \"scrcode: invalid argument type\"\n\
         .endif\n\
         scrcode a2, a3, a4, a5, a6, a7, a8, a9\n\
         .endmacro\n",
        string = of(".strat(a1, i)"),
        range = range,
        value = of("a1"),
    )
}

/// `module`: the header of a cc65 module, which is loaded at run time unless
/// `DYN_DRV` is defined as 0 first, when it is linked in.
const MODULE: &str = "\
.ifndef DYN_DRV
DYN_DRV = 1
.endif
.macro module_header label
.if DYN_DRV
.segment \"HEADER\"
.else
.data
.export label
label:
.endif
.endmacro
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_package_has_a_text() {
        for name in [
            "generic",
            "longbranch",
            "cpu",
            "cbm",
            "apple2",
            "atari",
            "module",
        ] {
            assert!(package(name).is_some_and(|text| !text.is_empty()), "{name}");
        }
        assert_eq!(package("nope"), None);
        assert!(longbranch().contains(".macro jvc target"));
        assert!(scrcode(APPLE2, false).contains(".byte (.strat(a1, i))+$80"));
        assert!(scrcode(CBM, true).contains(".if (a1) < 0 .or (a1) > 255"));
    }

    #[test]
    fn every_package_parses_cleanly() {
        for name in [
            "generic",
            "longbranch",
            "cpu",
            "cbm",
            "apple2",
            "atari",
            "module",
        ] {
            let text = package(name).expect("a package");
            let parse = crate::parse(&text, crate::Profile::CA65_2_19_6502).expect("parses");
            assert!(
                parse.diagnostics().is_empty(),
                "{name}: {:?}",
                parse.diagnostics()
            );
        }
    }

    /// A host that answers nothing, so `.macpack` reads this crate's own
    /// packages.
    struct Nothing;

    impl crate::expand::Host for Nothing {
        fn evaluate(&mut self, _: &crate::ast::Expr) -> Result<i64, String> {
            Ok(0)
        }
        fn is_defined(&mut self, _: &crate::ast::NameRef) -> bool {
            false
        }
        fn is_referenced(&mut self, _: &crate::ast::NameRef) -> bool {
            false
        }
        fn is_constant(&mut self, _: &crate::ast::Expr) -> bool {
            true
        }
    }

    #[test]
    fn macpack_reads_these_packages_by_default() {
        let source = ".macpack generic\n.macpack longbranch\n.macpack cbm\n\
                      .macpack module\n.macpack nosuch\n";
        let mut expander =
            crate::expand::Expander::new(source, crate::Profile::CA65_2_19_6502).expect("parses");
        while expander.next_line(&mut Nothing).is_some() {}
        let problems: Vec<_> = expander
            .diagnostics()
            .iter()
            .map(|(_, problem)| problem.message().to_owned())
            .collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("nosuch"), "{problems:?}");
    }

    #[test]
    fn the_cpu_constants_are_ca65s() {
        assert_eq!(PREDEFINED.len(), 28);
        assert_eq!(predefined("CPU_W65C02"), Some(0x1032));
        assert_eq!(predefined("CPU_ISET_NONE"), Some(1));
        assert_eq!(predefined("CPU_X"), None);
    }
}
