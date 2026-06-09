# Downstream Worked Example

Compiled model for downstream.

## Target

- system: <<system>>
- runtime ABI: nixfied-runtime-abi:1
- toolchain: nixfied-toolchain:1

## Surfaces

- model
- schema
- docs
- capabilities
- check
- run
- ps
- down
- clean

## Services

- api
- postgres
- worker

## Lifecycle

- api: readiness api-tcp; health explicit; operations prepare, start, ready, health, stop, clean
- postgres: readiness postgres-tcp; health explicit; operations prepare, start, ready, health, stop, clean
- worker: readiness worker-tcp; health explicit; operations prepare, start, ready, health, stop, clean

## Tasks

- ping-api
- ping-worker
- release-gate
- smoke-query
