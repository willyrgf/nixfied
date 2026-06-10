# Postgres Example

Compiled model for postgres-example.

## Target

- system: <<system>>
- runtime ABI: nixfied-runtime-abi:1-1b85992d183c
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

- postgres: endpoint postgres-tcp; operations clean, health, prepare, ready, start, stop

## Tasks

- smoke-query
