# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

### Removed

### Changed

### Fixed

## [0.2.0] - 2025-10-26

### Added

- occams-rpc:
    - Finish api interface client and server macro, and Inline dispatch

- oocams-rpc-stream:
    - Add ClientPool as connection pool
    - Add FailoverPool for high availability, which wraps user ClientFacts
    - Add ClientCaller and ClientCallerBlocking traits

- occams-rpc-core:
    - Add RpcErrCodec trait to support user custom error type
    - Add spawn_detach() to AsyncIO trait
    - Codec: Add encode_into()

- occams-rpc-tokio:
    - TokioRT now captures a runtime handle on new

- occams-rpc-smol:
    - SmolRT now support new_global() or new() with specified Executor

### Changed

- occams-rpc-stream:
    - Rename Factory -> Facts
    - Remove Transport from ClientFacts and ServerFacts (now depend on AsyncIO generic)
    - ClientFacts / ServerFacts now inherits AsyncIO trait
    - ServerFacts Removes RespTask and Codec, moved to Dispatch trait
    - Refactor ClientTask and ServerTask trait to support custom error types, and reduce alloc on encode.

- occams-rpc-tcp:
    - Optimise io with buffer

- occams-rpc-codec:
    - Adapt to new encode_into() interface
