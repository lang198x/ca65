# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.6...lang198x-ca65-v0.0.7) - 2026-10-27

### Added

- give ca65's remaining functions of tokens and symbols

## [0.0.6](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.5...lang198x-ca65-v0.0.6) - 2026-10-27

### Added

- report ca65 segment changes to the assembler

## [0.0.5](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.4...lang198x-ca65-v0.0.5) - 2026-10-27

### Added

- keep the bytes of a made string its text cannot hold
- parse each expansion from the tokens it was made of

### Fixed

- join a mid-line .endrepeat's line to the next, as ca65 does

## [0.0.4](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.3...lang198x-ca65-v0.0.4) - 2026-10-27

### Added

- [**breaking**] report ca65 scopes and lower .sizeof

## [0.0.3](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.2...lang198x-ca65-v0.0.3) - 2026-10-27

### Added

- locate a ca65 line in the files a person wrote
- lower whole ca65 lines to asm198x-ir
- lower ca65 expressions and instructions to asm198x-ir

### Fixed

- give ca65 lines the meaning Asm198x's verdicts recorded

## [0.0.2](https://github.com/lang198x/ca65/compare/lang198x-ca65-v0.0.1...lang198x-ca65-v0.0.2) - 2026-10-27

### Added

- read ca65 includes and macro packages in the expander
- run ca65 token functions in the expander
- expand ca65 define-style macros in the expander
- run ca65 repeats in the expander
- run ca65 macros in the expander
- run ca65 conditionals in an expander
- add typed views over the ca65 tree
- read each ca65 control command in its own shape
- parse ca65 lines, labels, instructions and expressions
