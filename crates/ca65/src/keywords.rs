//! ca65's control commands: the words that start with a dot.

/// Every control command ca65 V2.19 at `8d86ca7` knows, in upper case and
/// sorted, as its scanner's `DotKeywords` table lists them.
///
/// The table covers directives such as `.BYTE`, operators spelt as words
/// such as `.AND`, and functions such as `.LOBYTE`. ca65 ignores the case of
/// these words. This is the one copy of the list: Asm198x reads it from here
/// once its ca65 reader moves to this crate.
pub const CONTROL_COMMANDS: &[&str] = &[
    ".A16",
    ".A8",
    ".ADDR",
    ".ADDRSIZE",
    ".ALIGN",
    ".AND",
    ".ASCIIZ",
    ".ASIZE",
    ".ASSERT",
    ".AUTOIMPORT",
    ".BANK",
    ".BANKBYTE",
    ".BANKBYTES",
    ".BITAND",
    ".BITNOT",
    ".BITOR",
    ".BITXOR",
    ".BLANK",
    ".BSS",
    ".BYT",
    ".BYTE",
    ".CAP",
    ".CAPABILITY",
    ".CASE",
    ".CHARMAP",
    ".CODE",
    ".CONCAT",
    ".CONDES",
    ".CONST",
    ".CONSTRUCTOR",
    ".CPU",
    ".DATA",
    ".DBG",
    ".DBYT",
    ".DEBUGINFO",
    ".DEF",
    ".DEFINE",
    ".DEFINED",
    ".DEFINEDMACRO",
    ".DELMAC",
    ".DELMACRO",
    ".DESTRUCTOR",
    ".DWORD",
    ".ELSE",
    ".ELSEIF",
    ".END",
    ".ENDENUM",
    ".ENDIF",
    ".ENDMAC",
    ".ENDMACRO",
    ".ENDPROC",
    ".ENDREP",
    ".ENDREPEAT",
    ".ENDSCOPE",
    ".ENDSTRUCT",
    ".ENDUNION",
    ".ENUM",
    ".ERROR",
    ".EXITMAC",
    ".EXITMACRO",
    ".EXPORT",
    ".EXPORTZP",
    ".FARADDR",
    ".FATAL",
    ".FEATURE",
    ".FILEOPT",
    ".FOPT",
    ".FORCEIMPORT",
    ".FORCEWORD",
    ".GLOBAL",
    ".GLOBALZP",
    ".HIBYTE",
    ".HIBYTES",
    ".HIWORD",
    ".I16",
    ".I8",
    ".IDENT",
    ".IF",
    ".IFBLANK",
    ".IFCONST",
    ".IFDEF",
    ".IFNBLANK",
    ".IFNCONST",
    ".IFNDEF",
    ".IFNREF",
    ".IFP02",
    ".IFP02X",
    ".IFP4510",
    ".IFP45GS02",
    ".IFP6280",
    ".IFP816",
    ".IFPC02",
    ".IFPCE02",
    ".IFPDTV",
    ".IFPM740",
    ".IFPSC02",
    ".IFPSWEET16",
    ".IFPWC02",
    ".IFREF",
    ".IMPORT",
    ".IMPORTZP",
    ".INCBIN",
    ".INCLUDE",
    ".INTERRUPTOR",
    ".ISIZE",
    ".ISMNEM",
    ".ISMNEMONIC",
    ".LEFT",
    ".LINECONT",
    ".LIST",
    ".LISTBYTES",
    ".LITERAL",
    ".LOBYTE",
    ".LOBYTES",
    ".LOCAL",
    ".LOCALCHAR",
    ".LOWORD",
    ".MAC",
    ".MACPACK",
    ".MACRO",
    ".MATCH",
    ".MAX",
    ".MID",
    ".MIN",
    ".MOD",
    ".NOT",
    ".NULL",
    ".OR",
    ".ORG",
    ".OUT",
    ".P02",
    ".P02X",
    ".P4510",
    ".P45GS02",
    ".P6280",
    ".P816",
    ".PAGELEN",
    ".PAGELENGTH",
    ".PARAMCOUNT",
    ".PC02",
    ".PCE02",
    ".PDTV",
    ".PM740",
    ".POPCHARMAP",
    ".POPCPU",
    ".POPSEG",
    ".PROC",
    ".PSC02",
    ".PSWEET16",
    ".PUSHCHARMAP",
    ".PUSHCPU",
    ".PUSHSEG",
    ".PWC02",
    ".REF",
    ".REFERENCED",
    ".REFERTO",
    ".REFTO",
    ".RELOC",
    ".REPEAT",
    ".RES",
    ".RIGHT",
    ".RODATA",
    ".SCOPE",
    ".SEGMENT",
    ".SET",
    ".SETCPU",
    ".SHL",
    ".SHR",
    ".SIZEOF",
    ".SMART",
    ".SPRINTF",
    ".STRAT",
    ".STRING",
    ".STRLEN",
    ".STRUCT",
    ".TAG",
    ".TCOUNT",
    ".TIME",
    ".UNDEF",
    ".UNDEFINE",
    ".UNION",
    ".VERSION",
    ".WARNING",
    ".WORD",
    ".XMATCH",
    ".XOR",
    ".ZEROPAGE",
];

/// Whether `word`, with its leading dot, is a control command ca65 knows.
/// Case does not matter.
///
/// ```
/// use lang198x_ca65::is_control_command;
///
/// assert!(is_control_command(".byte"));
/// assert!(is_control_command(".LoByte"));
/// assert!(!is_control_command(".bogus"));
/// ```
#[must_use]
pub fn is_control_command(word: &str) -> bool {
    CONTROL_COMMANDS
        .binary_search_by(|known| cmp_upper(known, word))
        .is_ok()
}

/// Compares an upper-case table entry with `word`, as if `word` were upper
/// case too.
fn cmp_upper(known: &str, word: &str) -> std::cmp::Ordering {
    known
        .bytes()
        .cmp(word.bytes().map(|b| b.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_upper_case_and_complete() {
        // ca65 searches the table with `bsearch`, so it must be sorted, and
        // V2.19 at `8d86ca7` has 187 entries.
        assert!(CONTROL_COMMANDS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            CONTROL_COMMANDS
                .iter()
                .all(|word| word.starts_with('.') && *word == word.to_ascii_uppercase())
        );
        assert_eq!(CONTROL_COMMANDS.len(), 187);
    }

    #[test]
    fn every_entry_is_found_in_any_case() {
        for word in CONTROL_COMMANDS {
            assert!(is_control_command(word));
            assert!(is_control_command(&word.to_ascii_lowercase()));
        }
        assert!(!is_control_command("."));
        assert!(!is_control_command(".BYTES"));
        assert!(!is_control_command("BYTE"));
    }
}
