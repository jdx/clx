# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0](https://github.com/jdx/clx/compare/v1.2.0...v1.3.0) - 2026-01-31

### Added

- *(progress)* add multi-operation progress tracking ([#64](https://github.com/jdx/clx/pull/64))

## [1.2.0](https://github.com/jdx/clx/compare/v1.1.0...v1.2.0) - 2026-01-31

### Added

- add log crate integration for progress-aware logging ([#59](https://github.com/jdx/clx/pull/59))
- add SIGWINCH handling for terminal resize ([#57](https://github.com/jdx/clx/pull/57))
- add template functions and job query API ([#54](https://github.com/jdx/clx/pull/54))
- add environment variable controls for progress display ([#56](https://github.com/jdx/clx/pull/56))
- add convenience methods and ETA smoothing ([#55](https://github.com/jdx/clx/pull/55))
- *(progress)* add bytes() template function ([#49](https://github.com/jdx/clx/pull/49))

### Fixed

- *(ci)* remove rust-toolchain from release-plz workflow

### Other

- split progress.rs into modular directory structure ([#63](https://github.com/jdx/clx/pull/63))
- add unit tests for ETA/rate smoothing calculation ([#60](https://github.com/jdx/clx/pull/60))
- deduplicate refresh() and refresh_once() ([#62](https://github.com/jdx/clx/pull/62))
- add unit tests for error module ([#61](https://github.com/jdx/clx/pull/61))
- add unit tests for template functions ([#58](https://github.com/jdx/clx/pull/58))
- add comprehensive API documentation ([#51](https://github.com/jdx/clx/pull/51))
- document threading model for progress module ([#53](https://github.com/jdx/clx/pull/53))
- *(deps)* remove unused dependencies ([#50](https://github.com/jdx/clx/pull/50))

## [1.1.0](https://github.com/jdx/clx/compare/v1.0.0...v1.1.0) - 2026-01-31

### Added

- *(progress)* add hide_complete option to eta() and progress_bar() ([#47](https://github.com/jdx/clx/pull/47))
