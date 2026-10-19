---
title: "Implementation Phases — Detailed"
type: implementation
status: proposed
tags: [implementation, phases]
---

# Implementation Phases — Detailed

## Phase 1 work packages

### Controller core
- cluster state in memory;
- event bus;
- node registry;
- scheduler interface.

### Worker core
- heartbeat;
- capability report;
- CPU benchmark;
- Work Unit runner.

### CLI
- status;
- nodes;
- node show;
- events.

## Phase 2 work packages

- dependency DAG;
- queue;
- placement;
- network measurements;
- transfer protocol;
- retries;
- trace correlation.

## Phase 3 work packages

- image build;
- bootstrap;
- installer;
- bootloader;
- identity persistence;
- A/B recovery.

## Phase 4 work packages

- SSD/HDD/NVMe detection;
- SMART/health where available;
- dynamic cache allocation;
- swap manager;
- checkpoint manager.

## Phase 5 work packages

- graph ingestion;
- graph queries;
- retrieval evaluator;
- exact cache;
- semantic cache;
- compression evaluator.

## Phase 6 work packages

- model manifest;
- shard format;
- model cache;
- CPU kernel integration;
- distributed model execution.
