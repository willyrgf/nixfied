# Minimal

Compiled model for minimal.

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

- synthetic

## Lifecycle

- synthetic: readiness synthetic-tcp; health explicit; operations prepare, start, ready, health, stop, clean

## Tasks

- smoke
