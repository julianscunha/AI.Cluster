---
title: "Protocol Versioning"
type: protocol
status: draft
tags: [protocol, versioning]
---

# Protocol Versioning

## Rules

- Protobuf schema evolution must be backward compatible within supported version windows.
- Feature negotiation must occur during registration.
- Controller and Worker must report protocol/runtime versions separately.
- Incompatible nodes are rejected or quarantined before workload assignment.

## Upgrade strategy

Prefer rolling upgrades:

```text
node01 update
 -> validate
 -> node02
 -> validate
 -> ...
```

Never require the entire cluster to upgrade atomically unless explicitly declared as a major breaking release.
