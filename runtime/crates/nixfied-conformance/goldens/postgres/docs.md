# Postgres Example

Compiled model for postgres-example.

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

- postgres

## Lifecycle

- postgres: readiness postgres-tcp; health explicit; operations prepare, start, ready, health, stop, clean

## Tasks

- smoke-query
