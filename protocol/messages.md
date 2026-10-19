---
title: "Protocol Messages"
type: protocol
status: draft
tags: [protocol, messages]
---

# Protocol Messages

## Node registration

```json
{
  "node_id": "node-0017",
  "device_id": "dev-a81f",
  "runtime_version": "0.1.0",
  "os_version": "1.0.0",
  "architecture": "x86_64",
  "features": ["tensor-v1", "checkpoint-v1"]
}
```

## Work assignment

```json
{
  "work_id": "wu-1842",
  "execution_id": "exec-771",
  "plan_id": "plan-281",
  "type": "MATMUL",
  "precision": "INT4",
  "inputs": ["tensor://..."],
  "outputs": ["tensor://..."]
}
```

## Health report

```json
{
  "node_id": "node-17",
  "health": {
    "overall": 0.81,
    "thermal": 0.37,
    "storage": 0.41,
    "network": 0.98
  },
  "state": "DEGRADED"
}
```

Os schemas definitivos devem ser versionados via Protobuf.
