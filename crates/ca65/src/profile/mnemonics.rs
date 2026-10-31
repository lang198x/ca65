//! The mnemonics of each CPU, in upper case, as ca65 V2.19 at `8d86ca7`
//! lists them in `src/ca65/instr.c`, one table per CPU and in its order.

/// `InsTab6502`: 56 mnemonics.
pub(super) const MOS_6502: &[&str] = &[
    "ADC", "AND", "ASL", "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRK", "BVC", "BVS",
    "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY", "DEC", "DEX", "DEY", "EOR", "INC", "INX",
    "INY", "JMP", "JSR", "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PLA", "PLP",
    "ROL", "ROR", "RTI", "RTS", "SBC", "SEC", "SED", "SEI", "STA", "STX", "STY", "TAX", "TAY",
    "TSX", "TXA", "TXS", "TYA",
];

/// `InsTab6502X`: 75 mnemonics.
pub(super) const NMOS_6502X: &[&str] = &[
    "ADC", "ALR", "ANC", "AND", "ANE", "ARR", "ASL", "AXS", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRK", "BVC", "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY", "DCP",
    "DEC", "DEX", "DEY", "EOR", "INC", "INX", "INY", "ISC", "JAM", "JMP", "JSR", "LAS", "LAX",
    "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PLA", "PLP", "RLA", "ROL", "ROR",
    "RRA", "RTI", "RTS", "SAX", "SBC", "SEC", "SED", "SEI", "SHA", "SHX", "SHY", "SLO", "SRE",
    "STA", "STX", "STY", "TAS", "TAX", "TAY", "TSX", "TXA", "TXS", "TYA",
];

/// `InsTab6502DTV`: 71 mnemonics.
pub(super) const DTV_6502: &[&str] = &[
    "ADC", "ALR", "ANC", "AND", "ANE", "ARR", "ASL", "AXS", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRA", "BRK", "BVC", "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY",
    "DEC", "DEX", "DEY", "EOR", "INC", "INX", "INY", "JMP", "JSR", "LAS", "LAX", "LDA", "LDX",
    "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PLA", "PLP", "RLA", "ROL", "ROR", "RRA", "RTI",
    "RTS", "SAC", "SBC", "SEC", "SED", "SEI", "SHA", "SHX", "SHY", "SIR", "STA", "STX", "STY",
    "TAX", "TAY", "TSX", "TXA", "TXS", "TYA",
];

/// `InsTab65SC02`: 66 mnemonics.
pub(super) const MOS_65SC02: &[&str] = &[
    "ADC", "AND", "ASL", "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRA", "BRK", "BVC",
    "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY", "DEA", "DEC", "DEX", "DEY", "EOR",
    "INA", "INC", "INX", "INY", "JMP", "JSR", "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA",
    "PHP", "PHX", "PHY", "PLA", "PLP", "PLX", "PLY", "ROL", "ROR", "RTI", "RTS", "SBC", "SEC",
    "SED", "SEI", "STA", "STX", "STY", "STZ", "TAX", "TAY", "TRB", "TSB", "TSX", "TXA", "TXS",
    "TYA",
];

/// `InsTab65C02`: 98 mnemonics.
pub(super) const MOS_65C02: &[&str] = &[
    "ADC", "AND", "ASL", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5", "BBR6", "BBR7", "BBS0",
    "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRA", "BRK", "BVC", "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY",
    "DEA", "DEC", "DEX", "DEY", "EOR", "INA", "INC", "INX", "INY", "JMP", "JSR", "LDA", "LDX",
    "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PHX", "PHY", "PLA", "PLP", "PLX", "PLY", "RMB0",
    "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROR", "RTI", "RTS", "SBC",
    "SEC", "SED", "SEI", "SMB0", "SMB1", "SMB2", "SMB3", "SMB4", "SMB5", "SMB6", "SMB7", "STA",
    "STX", "STY", "STZ", "TAX", "TAY", "TRB", "TSB", "TSX", "TXA", "TXS", "TYA",
];

/// `InsTabW65C02`: 100 mnemonics.
pub(super) const WDC_W65C02: &[&str] = &[
    "ADC", "AND", "ASL", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5", "BBR6", "BBR7", "BBS0",
    "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRA", "BRK", "BVC", "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY",
    "DEA", "DEC", "DEX", "DEY", "EOR", "INA", "INC", "INX", "INY", "JMP", "JSR", "LDA", "LDX",
    "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PHX", "PHY", "PLA", "PLP", "PLX", "PLY", "RMB0",
    "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROR", "RTI", "RTS", "SBC",
    "SEC", "SED", "SEI", "SMB0", "SMB1", "SMB2", "SMB3", "SMB4", "SMB5", "SMB6", "SMB7", "STA",
    "STP", "STX", "STY", "STZ", "TAX", "TAY", "TRB", "TSB", "TSX", "TXA", "TXS", "TYA", "WAI",
];

/// `InsTab65CE02`: 133 mnemonics.
pub(super) const CSG_65CE02: &[&str] = &[
    "ADC", "AND", "ASL", "ASR", "ASW", "AUG", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5",
    "BBR6", "BBR7", "BBS0", "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS",
    "BEQ", "BIT", "BMI", "BNE", "BPL", "BRA", "BRK", "BSR", "BVC", "BVS", "CLC", "CLD", "CLE",
    "CLI", "CLV", "CMP", "CPX", "CPY", "CPZ", "DEA", "DEC", "DEW", "DEX", "DEY", "DEZ", "EOM",
    "EOR", "INA", "INC", "INW", "INX", "INY", "INZ", "JMP", "JSR", "LBCC", "LBCS", "LBEQ", "LBMI",
    "LBNE", "LBPL", "LBRA", "LBVC", "LBVS", "LDA", "LDX", "LDY", "LDZ", "LSR", "NEG", "NOP", "ORA",
    "PHA", "PHD", "PHP", "PHW", "PHX", "PHY", "PHZ", "PLA", "PLP", "PLX", "PLY", "PLZ", "RMB0",
    "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROR", "ROW", "RTI", "RTN",
    "RTS", "SBC", "SEC", "SED", "SEE", "SEI", "SMB0", "SMB1", "SMB2", "SMB3", "SMB4", "SMB5",
    "SMB6", "SMB7", "STA", "STX", "STY", "STZ", "TAB", "TAX", "TAY", "TAZ", "TBA", "TRB", "TSB",
    "TSX", "TSY", "TXA", "TXS", "TYA", "TYS", "TZA",
];

/// `InsTab4510`: 133 mnemonics.
pub(super) const CSG_4510: &[&str] = &[
    "ADC", "AND", "ASL", "ASR", "ASW", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5", "BBR6",
    "BBR7", "BBS0", "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS", "BEQ",
    "BIT", "BMI", "BNE", "BPL", "BRA", "BRK", "BSR", "BVC", "BVS", "CLC", "CLD", "CLE", "CLI",
    "CLV", "CMP", "CPX", "CPY", "CPZ", "DEA", "DEC", "DEW", "DEX", "DEY", "DEZ", "EOM", "EOR",
    "INA", "INC", "INW", "INX", "INY", "INZ", "JMP", "JSR", "LBCC", "LBCS", "LBEQ", "LBMI", "LBNE",
    "LBPL", "LBRA", "LBVC", "LBVS", "LDA", "LDX", "LDY", "LDZ", "LSR", "MAP", "NEG", "NOP", "ORA",
    "PHA", "PHD", "PHP", "PHW", "PHX", "PHY", "PHZ", "PLA", "PLP", "PLX", "PLY", "PLZ", "RMB0",
    "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROR", "ROW", "RTI", "RTN",
    "RTS", "SBC", "SEC", "SED", "SEE", "SEI", "SMB0", "SMB1", "SMB2", "SMB3", "SMB4", "SMB5",
    "SMB6", "SMB7", "STA", "STX", "STY", "STZ", "TAB", "TAX", "TAY", "TAZ", "TBA", "TRB", "TSB",
    "TSX", "TSY", "TXA", "TXS", "TYA", "TYS", "TZA",
];

/// `InsTab45GS02`: 149 mnemonics.
pub(super) const MEGA_45GS02: &[&str] = &[
    "ADC", "ADCQ", "AND", "ANDQ", "ASL", "ASLQ", "ASR", "ASRQ", "ASW", "BBR0", "BBR1", "BBR2",
    "BBR3", "BBR4", "BBR5", "BBR6", "BBR7", "BBS0", "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6",
    "BBS7", "BCC", "BCS", "BEQ", "BIT", "BITQ", "BMI", "BNE", "BPL", "BRA", "BRK", "BSR", "BVC",
    "BVS", "CLC", "CLD", "CLE", "CLI", "CLV", "CMP", "CMPQ", "CPX", "CPY", "CPZ", "DEA", "DEC",
    "DEQ", "DEW", "DEX", "DEY", "DEZ", "EOM", "EOR", "EORQ", "INA", "INC", "INQ", "INW", "INX",
    "INY", "INZ", "JMP", "JSR", "LBCC", "LBCS", "LBEQ", "LBMI", "LBNE", "LBPL", "LBRA", "LBVC",
    "LBVS", "LDA", "LDQ", "LDX", "LDY", "LDZ", "LSR", "LSRQ", "MAP", "NEG", "NOP", "ORA", "ORQ",
    "PHA", "PHD", "PHP", "PHW", "PHX", "PHY", "PHZ", "PLA", "PLP", "PLX", "PLY", "PLZ", "RMB0",
    "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROLQ", "ROR", "RORQ", "ROW",
    "RTI", "RTN", "RTS", "SBC", "SBCQ", "SEC", "SED", "SEE", "SEI", "SMB0", "SMB1", "SMB2", "SMB3",
    "SMB4", "SMB5", "SMB6", "SMB7", "STA", "STQ", "STX", "STY", "STZ", "TAB", "TAX", "TAY", "TAZ",
    "TBA", "TRB", "TSB", "TSX", "TSY", "TXA", "TXS", "TYA", "TYS", "TZA",
];

/// `InsTab65816`: 100 mnemonics.
pub(super) const WDC_65816: &[&str] = &[
    "ADC", "AND", "ASL", "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRA", "BRK", "BRL",
    "BVC", "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "COP", "CPA", "CPX", "CPY", "DEA", "DEC",
    "DEX", "DEY", "EOR", "INA", "INC", "INX", "INY", "JML", "JMP", "JSL", "JSR", "LDA", "LDX",
    "LDY", "LSR", "MVN", "MVP", "NOP", "ORA", "PEA", "PEI", "PER", "PHA", "PHB", "PHD", "PHK",
    "PHP", "PHX", "PHY", "PLA", "PLB", "PLD", "PLP", "PLX", "PLY", "REP", "ROL", "ROR", "RTI",
    "RTL", "RTS", "SBC", "SEC", "SED", "SEI", "SEP", "STA", "STP", "STX", "STY", "STZ", "SWA",
    "TAD", "TAS", "TAX", "TAY", "TCD", "TCS", "TDA", "TDC", "TRB", "TSA", "TSB", "TSC", "TSX",
    "TXA", "TXS", "TXY", "TYA", "TYX", "WAI", "WDM", "XBA", "XCE",
];

/// `InsTabSweet16`: 26 mnemonics.
pub(super) const SWEET16: &[&str] = &[
    "ADD", "BC", "BK", "BM", "BM1", "BNC", "BNM1", "BNZ", "BP", "BR", "BS", "BZ", "CPR", "DCR",
    "INR", "LD", "LDD", "POP", "POPD", "RS", "RTN", "SET", "ST", "STD", "STP", "SUB",
];

/// `InsTabHuC6280`: 135 mnemonics.
pub(super) const HUC6280: &[&str] = &[
    "ADC", "AND", "ASL", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5", "BBR6", "BBR7", "BBS0",
    "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRA", "BRK", "BSR", "BVC", "BVS", "CLA", "CLC", "CLD", "CLI", "CLV", "CLX",
    "CLY", "CMP", "CPX", "CPY", "CSH", "CSL", "DEA", "DEC", "DEX", "DEY", "EOR", "INA", "INC",
    "INX", "INY", "JMP", "JSR", "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA", "PHP", "PHX",
    "PHY", "PLA", "PLP", "PLX", "PLY", "RMB0", "RMB1", "RMB2", "RMB3", "RMB4", "RMB5", "RMB6",
    "RMB7", "ROL", "ROR", "RTI", "RTS", "SAX", "SAY", "SBC", "SEC", "SED", "SEI", "SET", "SMB0",
    "SMB1", "SMB2", "SMB3", "SMB4", "SMB5", "SMB6", "SMB7", "ST0", "ST1", "ST2", "STA", "STX",
    "STY", "STZ", "SXY", "TAI", "TAM", "TAM0", "TAM1", "TAM2", "TAM3", "TAM4", "TAM5", "TAM6",
    "TAM7", "TAX", "TAY", "TDD", "TIA", "TII", "TIN", "TMA", "TMA0", "TMA1", "TMA2", "TMA3",
    "TMA4", "TMA5", "TMA6", "TMA7", "TRB", "TSB", "TST", "TSX", "TXA", "TXS", "TYA",
];

/// `InsTabm740`: 106 mnemonics.
pub(super) const M740: &[&str] = &[
    "ADC", "AND", "ASL", "BBC0", "BBC1", "BBC2", "BBC3", "BBC4", "BBC5", "BBC6", "BBC7", "BBS0",
    "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7", "BCC", "BCS", "BEQ", "BIT", "BMI",
    "BNE", "BPL", "BRA", "BRK", "BVC", "BVS", "CLB0", "CLB1", "CLB2", "CLB3", "CLB4", "CLB5",
    "CLB6", "CLB7", "CLC", "CLD", "CLI", "CLT", "CLV", "CMP", "COM", "CPX", "CPY", "DEC", "DEX",
    "DEY", "EOR", "FST", "INC", "INX", "INY", "JMP", "JSR", "LDA", "LDM", "LDX", "LDY", "LSR",
    "NOP", "ORA", "PHA", "PHP", "PLA", "PLP", "RMB0", "RMB1", "RMB2", "RMB3", "RMB4", "RMB5",
    "RMB6", "RMB7", "ROL", "ROR", "RRF", "RTI", "RTS", "SBC", "SEB0", "SEB1", "SEB2", "SEB3",
    "SEB4", "SEB5", "SEB6", "SEB7", "SEC", "SED", "SEI", "SET", "SLW", "STA", "STP", "STX", "STY",
    "TAX", "TAY", "TST", "TSX", "TXA", "TXS", "TYA",
];
