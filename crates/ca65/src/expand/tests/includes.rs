//! Includes and packages, each case measured on ca65 V2.19 at `8d86ca7`
//! with these files beside the program.

use super::host::{TestHost, run_with};
use super::{Bytes, Refused, Want};
use crate::Profile;
use crate::ast::View;
use crate::expand::Expander;

const FILES: &[(&str, &str)] = &[
    ("a.inc", ".byte 1\n"),
    ("noeol.inc", ".byte 2"),
    ("b.inc", ".include \"c.inc\"\n"),
    ("c.inc", ".byte 3\n"),
    ("self.inc", ".include \"self.inc\"\n"),
    ("openif.inc", ".if 1\n"),
    ("closeif.inc", ".endif\n"),
    ("openmac.inc", ".macro m\n.byte 4\n"),
    ("closemac.inc", ".endmacro\n"),
    ("end.inc", ".byte 5\n.end\n.byte 6\n"),
    ("def.inc", ".define D 7\n"),
    ("rep.inc", ".repeat 2\n"),
    ("callm.inc", "m\n"),
    ("defm.inc", ".macro m\n.byte 8\n.endmacro\n"),
    ("com.inc", ".byte 1 ; c"),
    // ca65 reads a package as `name.mac` from its include path.
    ("pk.mac", ".macro m2\n.byte 4\n.endmacro\n"),
];

const MEASURED: &[(&str, Want)] = &[
    (".include \"a.inc\"\n.byte 9\n", Bytes(&[1, 9])),
    (".include \"noeol.inc\"\n.byte 9\n", Bytes(&[2, 9])),
    (".include \"com.inc\"\n.byte 9\n", Bytes(&[1, 9])),
    (".include \"b.inc\"\n", Bytes(&[3])),
    (".include \"end.inc\"\n.byte 9\n", Bytes(&[5])),
    (".include \"def.inc\"\n.byte D\n", Bytes(&[7])),
    (
        ".macro m\n.byte 8\n.endmacro\n.include \"callm.inc\"\n",
        Bytes(&[8]),
    ),
    (".include \"defm.inc\"\nm\n", Bytes(&[8])),
    // A file's name may come from a macro, a define or a function.
    (
        ".macro m f\n.include f\n.endmacro\n m \"a.inc\"\n",
        Bytes(&[1]),
    ),
    (".define F \"a.inc\"\n.include F\n", Bytes(&[1])),
    (".include .concat(\"a\", \".inc\")\n", Bytes(&[1])),
    // A skipped include is not opened.
    (
        ".if 0\n.include \"missing.inc\"\n.endif\n.byte 1\n",
        Bytes(&[1]),
    ),
    // ca65 reads tokens, so a body may run on past the end of a file, and a
    // file may close a conditional opened before it.
    (".include \"openmac.inc\"\n.endmacro\n", Bytes(&[])),
    (
        ".include \"rep.inc\"\n.byte 1\n.endrepeat\n",
        Bytes(&[1, 1]),
    ),
    (".if 1\n.include \"closeif.inc\"\n.byte 1\n", Bytes(&[1])),
    (".macpack pk\n m2\n", Bytes(&[4])),
    (
        ".include \"missing.inc\"\n.byte 9\n",
        Refused("there is no file `missing.inc`"),
    ),
    (".include \"A.INC\"\n", Refused("there is no file `A.INC`")),
    (".include \"self.inc\"\n", Refused("nest too deeply")),
    // But a conditional opened in a file is closed when the file ends.
    (
        ".include \"openif.inc\"\n.byte 1\n.endif\n",
        Refused("never closed with `.endif` in its file"),
    ),
    (
        ".include \"openif.inc\"\n.byte 1\n.endif\n",
        Refused("`.endif` with no `.if` open"),
    ),
    // A macro's body is not run, so its `.include` reads nothing.
    (
        ".macro m\n.byte 4\n.include \"closemac.inc\"\n",
        Refused("no `.endmacro`"),
    ),
    (".include\n", Refused("needs a file name")),
    (
        ".include \"a.inc\", 1\n",
        Refused("expected the end of the line"),
    ),
    (
        ".macpack nosuch\n",
        Refused("there is no macro package `nosuch`"),
    ),
    (".macpack\n", Refused("needs a package's name")),
    (".macpack pk\n.macpack pk\n", Refused("already defined")),
    (
        ".include \"a.inc\"\nzz\n",
        Refused("expected `:` after a name"),
    ),
];

#[test]
fn every_measured_include_matches_ca65() {
    for (source, want) in MEASURED {
        let got = run_with(source, FILES);
        match want {
            Bytes(bytes) => assert_eq!(got.as_deref(), Ok(*bytes), "{source:?}"),
            Refused(part) => {
                let problems = got.expect_err(source);
                assert!(
                    problems.iter().any(|p| p.contains(part)),
                    "{source:?}: {problems:?}"
                );
            }
        }
    }
}

#[test]
fn measured_includes_that_ca65_refuses_for_the_parser() {
    assert!(run_with(".include a.inc\n", FILES).is_err());
}

/// The expander and host after running `source` with [`FILES`].
fn ran(source: &str) -> (Expander, TestHost) {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(FILES);
    while let Some(line) = expander.next_line(&mut host) {
        host.assemble(&line);
    }
    (expander, host)
}

#[test]
fn each_file_read_in_is_numbered_and_asked_for_beside_its_includer() {
    let source = ".include \"b.inc\"\n.include \"a.inc\"\n";
    let (expander, host) = ran(source);
    let files: Vec<(u32, &str)> = expander
        .files()
        .iter()
        .map(|f| (f.id().get(), f.name()))
        .collect();
    assert_eq!(files, [(1, "b.inc"), (2, "c.inc"), (3, "a.inc")]);
    // `c.inc` is asked for from `b.inc`, file 1.
    let asked: Vec<(&str, u32)> = host.asked.iter().map(|(n, f)| (n.as_str(), *f)).collect();
    assert_eq!(asked, [("b.inc", 0), ("c.inc", 1), ("a.inc", 0)]);
    let site = expander.files()[0].site();
    assert_eq!(site.range().slice(source), Some(".include \"b.inc\""));
    assert!(expander.diagnostics().is_empty());
}

#[test]
fn an_included_line_that_is_expanded_says_it_was_included() {
    use lang198x_syntax::{Origin, TextId};
    let mut expander = Expander::new(
        ".define D 7\n.include \"use.inc\"\n",
        Profile::CA65_2_19_6502,
    )
    .expect("parses");
    let mut host = TestHost::with_files(&[("use.inc", ".byte D\n")]);
    let line = expander.next_line(&mut host).expect("the included line");
    assert_eq!(line.line().syntax().text().trim(), ".byte  7");
    let TextId::Expansion(id) = line.text() else {
        panic!("an expansion")
    };
    let byte = super::token(&line, ".byte");
    let origin = expander
        .provenance()
        .get(id)
        .and_then(|e| e.origin_at(byte.text_range().start()))
        .expect("an origin");
    let Origin::Included { from, site } = origin else {
        panic!("{origin:?}")
    };
    assert_eq!(from.text(), TextId::File(lang198x_syntax::FileId::new(1)));
    assert_eq!(site.range().start().get(), 12);
}
