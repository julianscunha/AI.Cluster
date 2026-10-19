# Next Tests

Status: VERIFIED means the check runs and its output was observed; PENDING means
it has not been executed. Same-host containers prove the mechanics, not the
cross-machine performance claim.

1. Controller registration with one worker. — VERIFIED (`ai-cluster nodes`)
2. Heartbeat loss detection. — VERIFIED (state flips READY -> OFFLINE after grace)
3. Capability report ingestion. — VERIFIED (threads/RAM/health reported by the worker)
4. Two-node TCP round trip. — VERIFIED with two workers on one host; separate
   machines still PENDING
5. Matrix benchmark single-node baseline. — PENDING (no local comparison mode yet)
6. Distributed matrix benchmark with two nodes. — VERIFIED on one host; real
   hardware still PENDING
7. Compare compute time, transfer time and total latency. — VERIFIED for the
   same-host case (`ai-cluster benchmark --size N --repetitions R`)
