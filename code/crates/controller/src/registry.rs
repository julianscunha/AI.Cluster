use ai_cluster_protocol::metadata::same_token;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_cluster_common::NodeState;
use ai_cluster_protocol::cluster::{CapabilityReport, HealthReport, NodeIdentity, NodeSnapshot};

/// Controller-observed clock. Node-supplied timestamps are never used for liveness.
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(text.as_bytes())
}

/// 128 random bits as hex. Without an entropy source the controller cannot make a secret
/// worth having, so it stops rather than issue a guessable one.
fn new_credential() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("sem fonte de entropia para a credencial do worker");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether the controller may hand new work to a node. A node that is leaving or believed dead
/// is not a candidate, so it never wins a unit; neither is one nobody approved yet (S2).
fn eligible(record: &NodeRecord) -> bool {
    record.trusted
        && !record.quarantined
        && !matches!(
            record.state,
            NodeState::Draining | NodeState::Quarantined | NodeState::Failed | NodeState::Offline
        )
}

#[derive(Clone)]
pub struct NodeRecord {
    pub identity: NodeIdentity,
    pub capability: CapabilityReport,
    pub health: HealthReport,
    pub state: NodeState,
    /// False until someone approves the node (S2). Liveness and health still flow for an
    /// untrusted node so an operator can see it; it just never receives work or state.
    pub trusted: bool,
    /// Set after repeated invalid results (G1): alive and visible, but never a candidate, like
    /// an untrusted node. Cleared only by an operator's `approve`. Not persisted: a controller
    /// restart forgives the node, which then re-earns the quarantine in a few more bad results.
    pub quarantined: bool,
    /// Invalid results in a row; a valid one resets it.
    pub strikes: u32,
    /// Secret issued at registration (S3b). Only the node that registered it knows it; the
    /// snapshot shown to readers leaves it out.
    pub credential: String,
    pub registered_at_unix_ms: u64,
    pub last_heartbeat_unix_ms: u64,
}

// The credential is a secret: a `{:?}` in a log or a failed assert must not print it.
impl std::fmt::Debug for NodeRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRecord")
            .field("identity", &self.identity)
            .field("state", &self.state)
            .field("trusted", &self.trusted)
            .field("quarantined", &self.quarantined)
            .field("credential", &"<redigido>")
            .finish_non_exhaustive()
    }
}

impl NodeRecord {
    pub fn snapshot(&self) -> NodeSnapshot {
        NodeSnapshot {
            identity: Some(self.identity.clone()),
            capability: Some(self.capability.clone()),
            health: Some(self.health),
            state: if self.quarantined {
                NodeState::Quarantined.as_str().to_string()
            } else {
                self.state.as_str().to_string()
            },
            registered_at_unix_ms: self.registered_at_unix_ms,
            last_heartbeat_unix_ms: self.last_heartbeat_unix_ms,
            trusted: self.trusted,
            attested: false,
            fingerprint: ai_cluster_protocol::metadata::credential_fingerprint(&self.credential),
            estimated_speed: crate::models::estimated_speed(
                self.capability.compute_score,
                self.capability.memory_bandwidth_loaded_score,
                self.capability.memory_bandwidth_score,
            ),
        }
    }
}

/// Resultado de [`Registry::approve_checked`].
#[derive(Debug, PartialEq, Eq)]
pub enum Approval {
    Approved,
    Unknown,
    /// A impressão digital não é a da credencial atual do worker.
    Mismatch,
}

/// What the controller remembers about a node beyond its live record (S5): the credential it
/// issued and whether the node was approved. It outlives `leave_node` and, when a file is
/// configured, a controller restart.
#[derive(Debug, Clone)]
struct Known {
    credential: String,
    trusted: bool,
    /// Survives `leave_node` and a restart, so neither is a way out of a quarantine.
    quarantined: bool,
    /// SHA-256 da chave pública do certificado de túnel já renovado (ADR-026); vazio = ainda não fixada.
    key: String,
}

#[derive(Debug)]
pub struct Registry {
    nodes: Mutex<HashMap<String, NodeRecord>>,
    known: Mutex<HashMap<String, Known>>,
    path: Option<std::path::PathBuf>,
    /// Whether a node that registers for the first time is trusted at once.
    auto_approve: bool,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// A registry that trusts every node on registration — the behaviour before S2.
    pub fn new() -> Self {
        Self {
            nodes: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            path: None,
            auto_approve: true,
        }
    }

    /// Keeps approvals and credentials in `path`, one `node_id<TAB>credential<TAB>0|1` line per
    /// node, so a restarted controller still recognizes the nodes it approved (S5). The file is
    /// private (0600) and holds secrets in clear; losing it only sends nodes back to pending.
    pub fn persisted(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                // Keeping going would overwrite a file we could not read with an empty one.
                eprintln!(
                    "aviso: não consegui ler {}: {err}; sem persistência",
                    path.display()
                );
                return self;
            }
        };
        if let Some(text) = text {
            let known = self.known.get_mut().unwrap_or_else(|e| e.into_inner());
            for line in text.lines() {
                let mut fields = line.split('\t');
                if let (Some(id), Some(credential), Some(trusted)) =
                    (fields.next(), fields.next(), fields.next())
                    && !id.is_empty()
                    && !credential.is_empty()
                {
                    known.insert(
                        id.to_string(),
                        Known {
                            credential: credential.to_string(),
                            trusted: trusted == "1",
                            // Optional fourth column; files from before G1 do not have it.
                            quarantined: fields.next() == Some("1"),
                            key: fields.next().unwrap_or("").to_string(),
                        },
                    );
                }
            }
        }
        self.path = Some(path);
        self
    }

    fn lock_known(&self) -> std::sync::MutexGuard<'_, HashMap<String, Known>> {
        self.known.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn remember(&self, node_id: &str, credential: &str, trusted: bool) {
        let mut known = self.lock_known();
        let (quarantined, key) = known.get(node_id).map_or((false, String::new()), |prior| {
            (prior.quarantined, prior.key.clone())
        });
        known.insert(
            node_id.to_string(),
            Known {
                credential: credential.to_string(),
                trusted,
                quarantined,
                key,
            },
        );
        self.save(&known);
    }

    /// Records the quarantine flag next to the credential, so it outlives the live record.
    fn remember_quarantine(&self, node_id: &str, quarantined: bool) {
        let mut known = self.lock_known();
        if let Some(entry) = known.get_mut(node_id) {
            entry.quarantined = quarantined;
            self.save(&known);
        }
    }

    fn save(&self, known: &HashMap<String, Known>) {
        let Some(path) = &self.path else { return };
        let mut ids: Vec<&String> = known.keys().collect();
        ids.sort();
        let text: String = ids
            .into_iter()
            .filter(|id| !id.contains(['\t', '\n']))
            .map(|id| {
                let entry = &known[id];
                format!(
                    "{id}\t{}\t{}\t{}\t{}\n",
                    entry.credential,
                    u8::from(entry.trusted),
                    u8::from(entry.quarantined),
                    entry.key
                )
            })
            .collect();
        let tmp = path.with_extension("tmp");
        // A leftover temp file would keep its old mode; `mode(0o600)` only applies on creation.
        let _ = std::fs::remove_file(&tmp);
        if let Err(err) = write_private(&tmp, &text).and_then(|_| std::fs::rename(&tmp, path)) {
            // Not fatal: the cost of a lost write is a node asking for approval again.
            eprintln!("aviso: não consegui gravar {}: {err}", path.display());
        }
    }

    /// Strips a node of trust and invalidates its credential (S5), whether or not it is live
    /// right now. False when the controller never knew it. This is also the rotation: a node
    /// that comes back registers again and receives a new secret.
    pub fn revoke(&self, node_id: &str) -> bool {
        let fresh = new_credential();
        let live = self
            .lock()
            .get_mut(node_id)
            .map(|record| {
                record.trusted = false;
                record.credential = fresh.clone();
                // frees the id: the next registration (rotation) is not read as a takeover
                record.state = NodeState::Offline;
            })
            .is_some();
        if live {
            self.remember(node_id, &fresh, false);
            return true;
        }
        let mut known = self.lock_known();
        let existed = known.remove(node_id).is_some();
        if existed {
            self.save(&known);
        }
        existed
    }

    /// Tira o worker do trabalho sem tirar a identidade: perde a confiança (não recebe unidades nem entra
    /// no plano de modelo), mas a credencial e a impressão digital continuam as mesmas. O worker segue
    /// conectado, como pendente; `approve` com a impressão digital de antes o põe de volta. False se o
    /// worker é desconhecido.
    pub fn retire(&self, node_id: &str) -> bool {
        // Ordem workers → known (a de `register_as`): o `approve` concorrente não vê vivo sem persistir.
        let mut nodes = self.lock();
        if let Some(record) = nodes.get_mut(node_id) {
            record.trusted = false;
            let credential = record.credential.clone();
            self.remember(node_id, &credential, false);
            return true;
        }
        let mut known = self.lock_known();
        let Some(entry) = known.get_mut(node_id) else {
            return false;
        };
        entry.trusted = false;
        self.save(&known);
        true
    }

    /// Esquece o worker por inteiro: registro vivo e credencial guardada. Se o worker continuar rodando, ele
    /// se apresenta de novo como um worker novo e pendente. False se o worker é desconhecido.
    pub fn forget(&self, node_id: &str) -> bool {
        let live = self.lock().remove(node_id).is_some();
        let mut known = self.lock_known();
        let had = known.remove(node_id).is_some();
        if had {
            self.save(&known);
        }
        live || had
    }

    /// Counts an invalid result against a node; at `limit` in a row it is quarantined.
    /// True only for the strike that quarantines it. False for an unknown node.
    pub fn strike(&self, node_id: &str, limit: u32) -> bool {
        let tripped = match self.lock().get_mut(node_id) {
            Some(record) => {
                record.strikes += 1;
                let tripped = record.strikes >= limit && !record.quarantined;
                if tripped {
                    record.quarantined = true;
                }
                tripped
            }
            None => false,
        };
        if tripped {
            self.remember_quarantine(node_id, true);
        }
        tripped
    }

    /// A valid result forgives the strikes before it.
    pub fn clear_strikes(&self, node_id: &str) {
        if let Some(record) = self.lock().get_mut(node_id) {
            record.strikes = 0;
        }
    }

    /// A registry where a new node is untrusted until `approve` is called (S2).
    pub fn requiring_approval() -> Self {
        Self {
            auto_approve: false,
            ..Self::new()
        }
    }

    /// Trusts a registered node. False when the node is unknown. Approving twice is fine.
    pub fn approve(&self, node_id: &str) -> bool {
        self.approve_if(node_id, |_| true) == Approval::Approved
    }

    /// Aprova sob um só lock se `allowed` aceitar o registro; a checagem e a aprovação não se separam.
    fn approve_if(&self, node_id: &str, allowed: impl Fn(&NodeRecord) -> bool) -> Approval {
        match self.lock().get_mut(node_id) {
            Some(record) if !allowed(record) => Approval::Mismatch,
            Some(record) => {
                record.trusted = true;
                // Approving is also how an operator releases a quarantined node.
                record.quarantined = false;
                record.strikes = 0;
                self.remember(node_id, &record.credential, true);
                self.remember_quarantine(node_id, false);
                Approval::Approved
            }
            None => Approval::Unknown,
        }
    }

    /// [`approve`](Self::approve) que só vale se a impressão digital da credencial atual do worker for
    /// `fingerprint`: quem tomou o id depois de um `revoke` tem outra credencial e não é aprovado.
    pub fn approve_checked(&self, node_id: &str, fingerprint: &str) -> Approval {
        self.approve_if(node_id, |record| {
            let current = ai_cluster_protocol::metadata::credential_fingerprint(&record.credential);
            same_token(current.as_bytes(), fingerprint.as_bytes())
        })
    }

    /// Upserts a node. Re-registration refreshes capability and liveness but
    /// preserves the original registration time.
    pub fn register(
        &self,
        identity: NodeIdentity,
        capability: CapabilityReport,
        now: u64,
    ) -> NodeRecord {
        self.register_as(identity, capability, now, b"")
            .expect("a node that is not approved can always register")
    }

    /// `register` for a caller that presents a credential. Only the holder of the credential
    /// issued for this `node_id` is that node coming back (S3b); anyone else — a restarted
    /// worker included, which lost it — gets a new credential and earns trust again, the safe
    /// direction to fail. It also supersedes the S2 device check, which a client could fake.
    ///
    /// With manual approval, an *approved* node is not replaced that way: `None`, so a holder of
    /// the join token cannot evict an approved node (and write that to disk) by registering its
    /// id. The operator runs `revoke` first, which makes the id free again. Under auto-approval
    /// trust is meaningless and a restarted worker must be able to come back, so it replaces.
    pub fn register_as(
        &self,
        identity: NodeIdentity,
        capability: CapabilityReport,
        now: u64,
        presented: &[u8],
    ) -> Option<NodeRecord> {
        let trusted = self.auto_approve;
        // Drawn before the lock: if entropy fails the panic must not poison the registry.
        let fresh = new_credential();
        let mut nodes = self.lock();
        if !self.auto_approve {
            let prior = nodes
                .get(&identity.node_id)
                .map(|record| {
                    (
                        record.trusted,
                        record.state != NodeState::Offline,
                        record.credential.clone(),
                    )
                })
                .or_else(|| {
                    self.lock_known()
                        .get(&identity.node_id)
                        .map(|known| (known.trusted, false, known.credential.clone()))
                });
            // An approved node, or a pending one that is still alive, is not replaced by someone who
            // does not hold its credential: the join token alone cannot take over an id.
            if let Some((trusted, live, credential)) = prior
                && (trusted || live)
                && !same_token(credential.as_bytes(), presented)
            {
                return None;
            }
        }
        // A node the controller knew before (it left, or the controller restarted) comes back
        // with the trust it earned when it proves its credential (S5): seed its record from what
        // was remembered and let the check below keep or reset it.
        if !nodes.contains_key(&identity.node_id)
            && let Some(prior) = self.lock_known().get(&identity.node_id).cloned()
        {
            nodes.insert(
                identity.node_id.clone(),
                NodeRecord {
                    identity: identity.clone(),
                    capability: capability.clone(),
                    health: HealthReport::default(),
                    state: NodeState::Offline,
                    trusted: prior.trusted,
                    quarantined: prior.quarantined,
                    strikes: 0,
                    credential: prior.credential,
                    registered_at_unix_ms: now,
                    last_heartbeat_unix_ms: now,
                },
            );
        }
        let record = nodes
            .entry(identity.node_id.clone())
            .and_modify(|record| {
                if !same_token(record.credential.as_bytes(), presented) {
                    record.credential = fresh.clone();
                    record.trusted = trusted;
                }
                record.identity = identity.clone();
                record.capability = capability.clone();
                record.state = NodeState::Ready;
                record.last_heartbeat_unix_ms = now;
            })
            .or_insert_with(|| NodeRecord {
                identity: identity.clone(),
                capability: capability.clone(),
                health: HealthReport::default(),
                state: NodeState::Ready,
                // Re-registration keeps the trust a node already earned (and_modify above).
                trusted,
                quarantined: false,
                strikes: 0,
                credential: fresh,
                registered_at_unix_ms: now,
                last_heartbeat_unix_ms: now,
            })
            .clone();
        self.remember(&identity.node_id, &record.credential, record.trusted);
        Some(record)
    }

    /// Whether `node_id` is registered and approved. Only such a node is handed controller state.
    pub fn is_trusted(&self, node_id: &str) -> bool {
        self.lock()
            .get(node_id)
            .is_some_and(|record| record.trusted)
    }

    /// Whether `node_id` may renew its tunnel certificate (ADR-026): it must still be approved and
    /// hold its credential. A revoked node is refused, and so is an unknown one (revoke forgets it
    /// when it is not live), so the join token alone never renews anything.
    pub fn renews(&self, node_id: &str, presented: &[u8]) -> bool {
        self.lock_known().get(node_id).is_some_and(|known| {
            known.trusted && same_token(known.credential.as_bytes(), presented)
        })
    }

    /// Fixa a chave pública do certificado do worker na primeira renovação e recusa outra nas seguintes
    /// (ADR-026). Quem chama já confirmou `renews`. Um worker esquecido ou revogado offline perde a fixação.
    pub fn bind_key(&self, node_id: &str, key_fingerprint: &str) -> bool {
        let mut known = self.lock_known();
        let Some(entry) = known.get_mut(node_id) else {
            return false;
        };
        if entry.key.is_empty() {
            entry.key = key_fingerprint.to_string();
        } else {
            return entry.key == key_fingerprint;
        }
        self.save(&known);
        true
    }

    /// Whether a call that speaks for `node_id` may proceed. An unknown node has no secret to
    /// protect, so it passes and the handler gives its usual "unknown" answer.
    pub fn authorizes(&self, node_id: &str, presented: &[u8]) -> bool {
        if let Some(record) = self.lock().get(node_id) {
            return same_token(record.credential.as_bytes(), presented);
        }
        // Not live: a node that left, or one the restarted controller has not heard from yet,
        // still owns its remembered credential.
        self.lock_known()
            .get(node_id)
            .is_none_or(|known| same_token(known.credential.as_bytes(), presented))
    }

    /// Whether `node_id` may open a resident channel (J5): registered, approved, alive and
    /// holding exactly this credential. Stricter than `authorizes`: an unknown node does not pass.
    pub fn channel_allowed(&self, node_id: &str, presented: &[u8]) -> bool {
        self.lock().get(node_id).is_some_and(|record| {
            eligible(record) && same_token(record.credential.as_bytes(), presented)
        })
    }

    /// The version and slot measurement a registered node reported at its last registration.
    pub fn version(&self, node_id: &str) -> Option<(String, String)> {
        self.lock().get(node_id).map(|record| {
            (
                record.identity.runtime_version.clone(),
                record.identity.image_sha256.clone(),
            )
        })
    }

    /// Whether the node declared it can apply a release (`can_update`).
    pub fn can_update(&self, node_id: &str) -> bool {
        self.lock()
            .get(node_id)
            .is_some_and(|record| record.identity.can_update)
    }

    /// Returns false when the node is unknown, so the caller can ask it to re-register.
    pub fn heartbeat(&self, node_id: &str, now: u64) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.last_heartbeat_unix_ms = now;
                if record.state == NodeState::Offline {
                    record.state = NodeState::Ready;
                }
                true
            }
            None => false,
        }
    }

    /// Records a health report. An optional capability profile travels beside it: when the
    /// node remeasured its compute while idle it sends a fresh profile, and the registry
    /// takes the new numbers while keeping the registration identity and timestamp (Fase C3).
    /// A capability of `None` means the node only reported health.
    pub fn report_health(
        &self,
        node_id: &str,
        health: HealthReport,
        capability: Option<CapabilityReport>,
    ) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.health = health;
                if let Some(capability) = capability {
                    record.capability = capability;
                }
                true
            }
            None => false,
        }
    }

    /// Whether the node is registered at all, regardless of its state.
    pub fn knows(&self, node_id: &str) -> bool {
        self.lock().contains_key(node_id)
    }

    /// Capability as last reported by the node. None when the node is unknown.
    pub fn capability(&self, node_id: &str) -> Option<CapabilityReport> {
        self.lock()
            .get(node_id)
            .map(|record| record.capability.clone())
    }

    /// Whether the controller may hand new work to this node. Unknown nodes and
    /// nodes leaving or believed dead are not eligible.
    pub fn is_available(&self, node_id: &str) -> bool {
        match self.lock().get(node_id) {
            Some(record) => eligible(record),
            None => false,
        }
    }

    /// Whether the node may keep serving inference slices. A draining node takes no new work but
    /// stays in the plan, so draining it does not restart the model and drop the sessions; a
    /// quarantined, failed or offline node is cut at once.
    pub fn serves_inference(&self, node_id: &str) -> bool {
        self.lock().get(node_id).is_some_and(|record| {
            record.trusted
                && !record.quarantined
                && !matches!(
                    record.state,
                    NodeState::Quarantined | NodeState::Failed | NodeState::Offline
                )
        })
    }

    /// Whether the node is registered but not yet approved (S2). Such a node is alive and polls
    /// for work; it is told there is none, not that it is unavailable, so its worker keeps
    /// running until someone approves it.
    pub fn awaiting_approval(&self, node_id: &str) -> bool {
        self.lock()
            .get(node_id)
            .is_some_and(|record| !record.trusted)
    }

    /// Marks a known node as draining so no further work is dispatched to it.
    /// Returns false when the node is unknown, which means it already left.
    pub fn begin_drain(&self, node_id: &str) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.state = NodeState::Draining;
                true
            }
            None => false,
        }
    }

    /// Removes a node from the registry. Returns false when it was unknown. A quarantined node
    /// stays listed (leaving is not a way to make the quarantine invisible); it counts as known.
    pub fn remove(&self, node_id: &str) -> bool {
        let mut nodes = self.lock();
        if nodes.get(node_id).is_some_and(|record| record.quarantined) {
            return true;
        }
        nodes.remove(node_id).is_some()
    }

    pub fn list(&self) -> Vec<NodeSnapshot> {
        let nodes = self.lock();
        let mut ids: Vec<&String> = nodes.keys().collect();
        ids.sort();
        ids.into_iter().map(|id| nodes[id].snapshot()).collect()
    }

    /// Available nodes as `(node_id, capability, health)`, sorted by id. This is the
    /// candidate set placement ranks over, read in one lock so a node cannot change
    /// state halfway through the ranking.
    pub fn candidates(&self) -> Vec<(String, CapabilityReport, HealthReport)> {
        let nodes = self.lock();
        let mut ids: Vec<&String> = nodes.keys().collect();
        ids.sort();
        ids.into_iter()
            .filter(|id| eligible(&nodes[*id]))
            .map(|id| {
                let record = &nodes[id];
                (id.clone(), record.capability.clone(), record.health)
            })
            .collect()
    }

    /// Registered node count and how many of them are available for work, read in one
    /// lock so the two numbers cannot come from different states of the world.
    pub fn counts(&self) -> (u32, u32) {
        let nodes = self.lock();
        let total = nodes.len() as u32;
        let available = nodes.values().filter(|record| eligible(record)).count() as u32;
        (total, available)
    }

    /// Marks nodes that missed `grace_ms` of controller-observed heartbeats as
    /// offline and returns the ids that transitioned.
    pub fn mark_stale_offline(&self, now: u64, grace_ms: u64) -> Vec<String> {
        let mut nodes = self.lock();
        let mut transitioned = Vec::new();
        for record in nodes.values_mut() {
            if record.state == NodeState::Offline {
                continue;
            }
            if now.saturating_sub(record.last_heartbeat_unix_ms) >= grace_ms {
                record.state = NodeState::Offline;
                transitioned.push(record.identity.node_id.clone());
            }
        }
        transitioned.sort();
        transitioned
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, NodeRecord>> {
        self.nodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(node_id: &str) -> NodeIdentity {
        NodeIdentity {
            node_id: node_id.to_string(),
            device_id: format!("{node_id}-device"),
            architecture: "x86_64".to_string(),
            os_version: "linux".to_string(),
            image_sha256: String::new(),
            can_update: false,
            runtime_version: "0.1.0".to_string(),
        }
    }

    #[test]
    fn approve_checked_refuses_a_node_whose_id_was_taken_after_a_revoke() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("n1"), CapabilityReport::default(), 1);
        let seen = ai_cluster_protocol::metadata::credential_fingerprint(&first.credential);
        registry.revoke("n1");
        // O intruso, só com o token de join, registra o mesmo id antes da aprovação.
        let taken = registry
            .register_as(identity("n1"), CapabilityReport::default(), 2, b"")
            .unwrap();
        assert_ne!(first.credential, taken.credential);
        assert_eq!(registry.approve_checked("n1", &seen), Approval::Mismatch);
        assert!(!registry.is_trusted("n1"));
        let theirs = ai_cluster_protocol::metadata::credential_fingerprint(&taken.credential);
        assert_eq!(registry.approve_checked("n1", &theirs), Approval::Approved);
        assert_eq!(
            registry.approve_checked("ghost", &theirs),
            Approval::Unknown
        );
    }

    #[test]
    fn retirar_tira_do_trabalho_e_a_impressao_de_antes_devolve_o_no() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("n1"), CapabilityReport::default(), 1);
        let seen = ai_cluster_protocol::metadata::credential_fingerprint(&first.credential);
        assert_eq!(registry.approve_checked("n1", &seen), Approval::Approved);

        assert!(registry.retire("n1"));
        assert!(!registry.is_trusted("n1"));
        // A identidade fica: mesma credencial, mesma impressão digital.
        assert_eq!(registry.list()[0].fingerprint, seen);
        assert!(registry.authorizes("n1", first.credential.as_bytes()));
        // O worker que volta com a credencial dele continua o mesmo worker, ainda fora do trabalho.
        let back = registry
            .register_as(
                identity("n1"),
                CapabilityReport::default(),
                2,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert_eq!(back.credential, first.credential);
        assert!(!registry.is_trusted("n1"));
        assert_eq!(registry.approve_checked("n1", &seen), Approval::Approved);
        assert!(registry.is_trusted("n1"));
        assert!(!registry.retire("ghost"));
    }

    #[test]
    fn remover_esquece_o_no_e_a_credencial() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("n1"), CapabilityReport::default(), 1);
        let seen = ai_cluster_protocol::metadata::credential_fingerprint(&first.credential);
        registry.approve_checked("n1", &seen);
        assert!(registry.forget("n1"));
        assert!(registry.list().is_empty());
        assert!(!registry.forget("n1"), "esquecer duas vezes: desconhecido");
        // Quem volta é um worker novo e pendente, com outra credencial.
        let again = registry.register(identity("n1"), CapabilityReport::default(), 2);
        assert_ne!(again.credential, first.credential);
        assert!(!registry.is_trusted("n1"));
    }

    #[test]
    fn repeated_invalid_results_quarantine_a_node_until_approved_again() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(!registry.strike("node-a", 3));
        registry.clear_strikes("node-a");
        // A valid result in between forgave the earlier strike.
        assert!(!registry.strike("node-a", 3));
        assert!(!registry.strike("node-a", 3));
        assert!(registry.is_available("node-a"));
        assert!(registry.strike("node-a", 3));
        assert!(!registry.is_available("node-a"));
        assert_eq!(registry.list()[0].state, "QUARANTINED");
        // Re-registering does not cure it, and neither does leaving first.
        registry.register(identity("node-a"), CapabilityReport::default(), 2_000);
        assert!(!registry.is_available("node-a"));
        registry.remove("node-a");
        registry.register(identity("node-a"), CapabilityReport::default(), 3_000);
        assert!(!registry.is_available("node-a"));
        // Only an operator's approve releases it.
        assert!(registry.approve("node-a"));
        assert!(registry.is_available("node-a"));
        assert_ne!(registry.list()[0].state, "QUARANTINED");
    }

    #[test]
    fn a_new_node_waits_for_approval_and_keeps_it_across_re_registration() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        // Untrusted: alive and visible, but never a candidate or counted as available.
        assert!(registry.knows("node-a"));
        assert!(!registry.is_available("node-a"));
        assert!(registry.awaiting_approval("node-a"));
        assert!(!registry.awaiting_approval("ghost"));
        assert!(registry.candidates().is_empty());
        assert_eq!(registry.counts(), (1, 0));
        assert!(!registry.list()[0].trusted);

        assert!(registry.approve("node-a"));
        assert!(registry.is_available("node-a"));
        assert_eq!(registry.counts(), (1, 1));

        // Re-registering with its own credential (a lost heartbeat) must not revoke the approval.
        registry
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                2_000,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert!(registry.is_available("node-a"));
    }

    #[test]
    fn um_no_pendente_vivo_nao_perde_o_id_para_quem_so_tem_o_token_de_join() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);

        // Vivo e ainda não aprovado: um intruso com outra credencial não o toma.
        let intruder = registry.register_as(
            identity("node-a"),
            CapabilityReport::default(),
            1_500,
            b"intruso",
        );
        assert!(intruder.is_none());
        assert!(registry.authorizes("node-a", first.credential.as_bytes()));

        // O dono volta com a própria credencial.
        assert!(
            registry
                .register_as(
                    identity("node-a"),
                    CapabilityReport::default(),
                    1_600,
                    first.credential.as_bytes(),
                )
                .is_some()
        );

        // Sem sinal, o id fica livre: um worker que perdeu a credencial volta com uma nova.
        registry.mark_stale_offline(60_000, 15_000);
        let again = registry
            .register_as(identity("node-a"), CapabilityReport::default(), 60_000, b"")
            .unwrap();
        assert_ne!(again.credential, first.credential);
    }
    #[test]
    fn the_same_node_id_without_the_credential_does_not_inherit_trust() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.approve("node-a");

        // The holder comes back: same credential, trust kept.
        let back = registry
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                1_500,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert!(back.trusted);
        assert_eq!(back.credential, first.credential);

        // Anyone else cannot take an approved node's id, device_id copied or not.
        let refused = registry.register_as(
            identity("node-a"),
            CapabilityReport::default(),
            2_000,
            b"guess",
        );
        assert!(refused.is_none());
        assert!(registry.authorizes("node-a", first.credential.as_bytes()));

        // After `revoke` the id is free: the newcomer gets a new secret and no trust.
        assert!(registry.revoke("node-a"));
        let newcomer = registry
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                3_000,
                b"guess",
            )
            .unwrap();
        assert!(!newcomer.trusted);
        assert_ne!(newcomer.credential, first.credential);
        assert!(!registry.authorizes("node-a", first.credential.as_bytes()));
        assert!(registry.authorizes("node-a", newcomer.credential.as_bytes()));
        assert!(registry.authorizes("ghost", b""));
    }

    #[test]
    fn approving_an_unknown_node_is_false_and_the_default_registry_trusts_everyone() {
        let registry = Registry::requiring_approval();
        assert!(!registry.approve("ghost"));
        let open = Registry::new();
        open.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(open.is_available("node-a"));
    }

    #[test]
    fn register_heartbeat_and_health_round_trip() {
        let registry = Registry::new();
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 8,
                ..Default::default()
            },
            1_000,
        );

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, "READY");
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 8);

        assert!(registry.heartbeat("node-a", 2_000));
        assert_eq!(registry.list()[0].last_heartbeat_unix_ms, 2_000);

        let health = HealthReport {
            health_score: 0.75,
            ..Default::default()
        };
        assert!(registry.report_health("node-a", health, None));
        assert_eq!(
            registry.list()[0].health.as_ref().unwrap().health_score,
            0.75
        );
    }

    #[test]
    fn unknown_node_is_rejected_without_registering() {
        let registry = Registry::new();
        assert!(!registry.heartbeat("ghost", 1_000));
        assert!(!registry.report_health("ghost", HealthReport::default(), None));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn a_health_report_can_refresh_the_capability() {
        let registry = Registry::new();
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 4,
                compute_score: 2.0,
                ..Default::default()
            },
            1_000,
        );

        // Health only: the registered profile survives untouched.
        assert!(registry.report_health("node-a", HealthReport::default(), None));
        assert_eq!(
            registry.list()[0]
                .capability
                .as_ref()
                .unwrap()
                .compute_score,
            2.0
        );

        // With a profile: the current numbers land in the registry while the baseline and
        // the registration identity stay put (Fase C3).
        let refreshed = CapabilityReport {
            cpu_threads: 4,
            compute_score: 2.0,
            current_compute_score: 1.5,
            compute_confidence: 0.8,
            ..Default::default()
        };
        assert!(registry.report_health("node-a", HealthReport::default(), Some(refreshed)));
        let stored = registry.list()[0].capability.clone().unwrap();
        assert_eq!(stored.compute_score, 2.0);
        assert_eq!(stored.current_compute_score, 1.5);
        assert_eq!(stored.compute_confidence, 0.8);
        // The refresh must not look like a re-registration.
        assert_eq!(registry.list()[0].registered_at_unix_ms, 1_000);
    }

    #[test]
    fn stale_node_goes_offline_and_recovers() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);

        assert!(registry.mark_stale_offline(1_500, 10_000).is_empty());
        assert_eq!(registry.list()[0].state, "READY");

        assert_eq!(
            registry.mark_stale_offline(11_000, 10_000),
            vec!["node-a".to_string()]
        );
        assert_eq!(registry.list()[0].state, "OFFLINE");

        assert!(registry.heartbeat("node-a", 12_000));
        assert_eq!(registry.list()[0].state, "READY");
    }

    #[test]
    fn re_registration_preserves_registration_time() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 16,
                ..Default::default()
            },
            5_000,
        );

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].registered_at_unix_ms, 1_000);
        assert_eq!(listed[0].last_heartbeat_unix_ms, 5_000);
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 16);
    }

    #[test]
    fn draining_node_stops_taking_work_and_can_be_removed() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(registry.is_available("node-a"));

        assert!(registry.begin_drain("node-a"));
        assert!(!registry.is_available("node-a"));
        assert!(
            registry.serves_inference("node-a"),
            "drain não derruba a inferência"
        );
        assert_eq!(registry.list()[0].state, "DRAINING");

        // Draining must survive a heartbeat; the node is leaving, not recovering.
        assert!(registry.heartbeat("node-a", 2_000));
        assert_eq!(registry.list()[0].state, "DRAINING");

        assert!(registry.remove("node-a"));
        assert!(!registry.knows("node-a"));
        assert!(registry.list().is_empty());

        // Leaving twice is not an error.
        assert!(!registry.begin_drain("node-a"));
        assert!(!registry.remove("node-a"));
    }

    #[test]
    fn offline_and_unknown_nodes_are_not_available_for_work() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(registry.is_available("node-a"));

        registry.mark_stale_offline(11_000, 10_000);
        assert!(!registry.is_available("node-a"));
        assert!(!registry.is_available("ghost"));
    }

    #[test]
    fn a_quarantine_survives_a_controller_restart() {
        let path = std::env::temp_dir().join(format!("aic-quarantine-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let registry = Registry::new().persisted(&path);
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(registry.strike("node-a", 1));

        let restarted = Registry::new().persisted(&path);
        let back = restarted
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                2_000,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert!(back.quarantined);
        assert!(!restarted.is_available("node-a"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn approval_survives_a_leave_and_a_restart_for_the_holder_of_the_credential() {
        let path = std::env::temp_dir().join(format!("aic-nodes-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let registry = Registry::requiring_approval().persisted(&path);
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.approve("node-a");

        // Planned departure, then the same worker comes back with its credential.
        registry.remove("node-a");
        let back = registry
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                2_000,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert!(back.trusted);

        // A new controller process reads the file; only the holder is recognized.
        let restarted = Registry::requiring_approval().persisted(&path);
        assert!(restarted.authorizes("node-a", first.credential.as_bytes()));
        assert!(!restarted.authorizes("node-a", b"guess"));
        let again = restarted
            .register_as(
                identity("node-a"),
                CapabilityReport::default(),
                3_000,
                first.credential.as_bytes(),
            )
            .unwrap();
        assert!(again.trusted);
        let stranger =
            restarted.register_as(identity("node-a"), CapabilityReport::default(), 4_000, b"");
        assert!(stranger.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn renewal_needs_an_approved_node_with_its_credential_and_revoke_refuses_it() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        let credential = first.credential.clone();
        // pendente: nem o token de join nem a credencial renovam
        assert!(!registry.renews("node-a", credential.as_bytes()));
        registry.approve("node-a");
        assert!(registry.renews("node-a", credential.as_bytes()));
        assert!(!registry.renews("node-a", b"outra"));
        assert!(!registry.renews("ghost", b""));
        assert!(registry.revoke("node-a"));
        assert!(!registry.renews("node-a", credential.as_bytes()));
        // revogado e saiu: o id foi esquecido, e um desconhecido também não renova
        registry.approve("node-a");
        registry.remove("node-a");
        assert!(registry.revoke("node-a"));
        assert!(!registry.renews("node-a", credential.as_bytes()));
    }

    #[test]
    fn a_renewal_key_is_pinned_on_first_use_and_kept_through_a_live_revoke() {
        let registry = Registry::requiring_approval();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.approve("node-a");
        assert!(registry.bind_key("node-a", "chave-1"));
        assert!(registry.bind_key("node-a", "chave-1"));
        assert!(!registry.bind_key("node-a", "chave-2"));
        // revogar (worker vivo) não solta a fixação: a chave antiga não volta a ser aceita como nova
        assert!(registry.revoke("node-a"));
        registry.approve("node-a");
        assert!(!registry.bind_key("node-a", "chave-2"));
        assert!(!registry.bind_key("ghost", "chave-1"));
    }

    #[test]
    fn revoking_a_node_returns_it_to_pending_and_kills_its_credential() {
        let registry = Registry::requiring_approval();
        let first = registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.approve("node-a");
        assert!(registry.revoke("node-a"));
        assert!(registry.awaiting_approval("node-a"));
        assert!(!registry.authorizes("node-a", first.credential.as_bytes()));
        // Revoking also covers a node that already left.
        registry.approve("node-a");
        registry.remove("node-a");
        assert!(registry.revoke("node-a"));
        assert!(!registry.revoke("ghost"));
    }
}
