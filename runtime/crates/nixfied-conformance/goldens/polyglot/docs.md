# Polyglot Stack

Compiled model for polyglot-stack.

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

- api
- worker

## Lifecycle

- api: endpoint api-tcp; operations clean, health, prepare, ready, start, stop
- worker: endpoint worker-tcp; operations clean, health, prepare, ready, start, stop

## Tasks

- ping-api
- ping-worker
