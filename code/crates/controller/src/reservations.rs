//! Reservas de nós para execução futura (G6).
//!
//! Uma reserva retém um nó para um `request_id` **antes** de o trabalho existir. O valor
//! está em fixar afinidade: um cliente que vai enfileirar um pedido logo reserva o nó onde
//! os dados dele já estão, e enquanto a reserva vale nenhuma outra requisição pode usar
//! esse nó. Sem reserva, o nó ficaria disponível e qualquer requisição o tomaria.
//!
//! A reserva não empurra trabalho — quem puxa é o worker. Ela só restringe quem *pode*
//! receber um nó: a requisição dona, quando seu worker fizer poll. Um cliente que reserva e
//! nunca envia nada deixa de segurar o nó quando o TTL vence.
//!
//! A reserva guarda o nó inteiro (`slots = 0`) ou só alguns dos seus slots de trabalho
//! (`work_slots`). A unidade de capacidade é o slot porque é por ele que o placement admite
//! trabalho (`in_flight < work_slots`) e divide os núcleos: outra requisição só usa o nó enquanto
//! sobrar slot além dos guardados. Sem aplicação no worker, é contabilidade de admissão.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// TTL padrão de uma reserva quando o cliente não informa um (60s).
pub const DEFAULT_TTL_MS: u64 = 60_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Reservation {
    pub request_id: String,
    pub expires_at_unix_ms: u64,
    /// Slots guardados para a requisição dona; 0 é o nó inteiro.
    pub slots: u32,
}

/// Quem retém um nó agora e quanto dele.
#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
    pub request_id: String,
    pub slots: u32,
}

impl Hold {
    /// Se uma unidade de `request_id` pode usar o nó apesar da reserva. A dona sempre pode. As
    /// outras só quando a reserva é parcial e, somadas às unidades em voo, ainda deixam um slot
    /// livre além dos guardados. Reserva do nó inteiro (0 slots) não admite ninguém.
    pub fn admits(&self, request_id: &str, in_flight: u32, work_slots: u32) -> bool {
        self.request_id == request_id
            || (self.slots > 0 && in_flight.saturating_add(self.slots) < work_slots.max(1))
    }
}

#[derive(Debug, Default)]
pub struct Reservations {
    held: Mutex<HashMap<String, Reservation>>,
}

impl Reservations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retém um nó para um `request_id` por `ttl_ms` a partir de `now`.
    ///
    /// Uma reserva nova sobre o mesmo nó substitui a anterior: um nó, uma reserva. TTL
    /// zero usa o padrão; um TTL explícito é respeitado como veio.
    pub fn reserve(&self, node_id: &str, request_id: &str, ttl_ms: u64, now: u64) -> Reservation {
        self.reserve_slots(node_id, request_id, 0, ttl_ms, now)
    }

    /// Como [`Self::reserve`], guardando só `slots` slots do nó (0 é o nó inteiro).
    pub fn reserve_slots(
        &self,
        node_id: &str,
        request_id: &str,
        slots: u32,
        ttl_ms: u64,
        now: u64,
    ) -> Reservation {
        let ttl = if ttl_ms == 0 { DEFAULT_TTL_MS } else { ttl_ms };
        let reservation = Reservation {
            request_id: request_id.to_string(),
            expires_at_unix_ms: now.saturating_add(ttl),
            slots,
        };
        self.lock().insert(node_id.to_string(), reservation.clone());
        reservation
    }

    /// Libera a reserva de um nó. Devolve false quando não havia reserva ativa.
    pub fn release(&self, node_id: &str) -> bool {
        self.lock().remove(node_id).is_some()
    }

    /// A requisição para quem o nó está retido agora, ou None quando livre ou expirado.
    ///
    /// Uma reserva vencida é descartada na leitura, então o nó volta a ser candidato sem
    /// nenhum timer de fundo.
    pub fn held_for(&self, node_id: &str, now: u64) -> Option<String> {
        self.hold(node_id, now).map(|hold| hold.request_id)
    }

    /// Quem retém o nó e quantos slots, ou None quando livre ou expirado.
    pub fn hold(&self, node_id: &str, now: u64) -> Option<Hold> {
        let mut held = self.lock();
        match held.get(node_id) {
            Some(reservation) if reservation.expires_at_unix_ms > now => Some(Hold {
                request_id: reservation.request_id.clone(),
                slots: reservation.slots,
            }),
            Some(_) => {
                held.remove(node_id);
                None
            }
            None => None,
        }
    }

    /// Troca todas as reservas pelas de um snapshot carregado, descartando as já vencidas.
    pub fn restore(&self, entries: impl IntoIterator<Item = (String, Reservation)>, now: u64) {
        let mut held = self.lock();
        held.clear();
        held.extend(
            entries
                .into_iter()
                .filter(|(_, reservation)| reservation.expires_at_unix_ms > now),
        );
    }

    /// Reservas ativas, ordenadas por nó. As vencidas são descartadas na leitura.
    pub fn list(&self, now: u64) -> Vec<(String, Reservation)> {
        let mut held = self.lock();
        held.retain(|_, reservation| reservation.expires_at_unix_ms > now);
        let mut ids: Vec<&String> = held.keys().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| (id.clone(), held[id].clone()))
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Reservation>> {
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reserved_node_is_held_for_its_request() {
        let reservations = Reservations::new();
        let held = reservations.reserve("node-a", "req-x", 5_000, 1_000);

        assert_eq!(held.expires_at_unix_ms, 6_000);
        assert_eq!(
            reservations.held_for("node-a", 2_000),
            Some("req-x".to_string())
        );
        // A node nobody reserved is free.
        assert_eq!(reservations.held_for("node-b", 2_000), None);
    }

    #[test]
    fn a_zero_ttl_falls_back_to_the_default() {
        let reservations = Reservations::new();
        let held = reservations.reserve("node-a", "req-x", 0, 1_000);
        assert_eq!(held.expires_at_unix_ms, 1_000 + DEFAULT_TTL_MS);
    }

    #[test]
    fn an_expired_reservation_frees_the_node_on_read() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);

        // Still inside the window.
        assert_eq!(
            reservations.held_for("node-a", 5_999),
            Some("req-x".to_string())
        );
        // Past it: the hold is gone, and the entry is dropped so it cannot come back.
        assert_eq!(reservations.held_for("node-a", 6_000), None);
        assert_eq!(reservations.list(6_000).len(), 0);
    }

    #[test]
    fn release_frees_a_held_node_and_reports_whether_it_did() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);

        assert!(reservations.release("node-a"));
        assert_eq!(reservations.held_for("node-a", 2_000), None);
        // Releasing a node nobody held is not an error.
        assert!(!reservations.release("node-a"));
    }

    #[test]
    fn a_second_reservation_replaces_the_first() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);
        reservations.reserve("node-a", "req-y", 9_000, 1_000);

        assert_eq!(
            reservations.held_for("node-a", 2_000),
            Some("req-y".to_string())
        );
        assert_eq!(reservations.list(2_000).len(), 1);
    }

    #[test]
    fn uma_reserva_parcial_deixa_slot_para_os_outros() {
        let reservations = Reservations::new();
        reservations.reserve_slots("node-a", "req-x", 1, 5_000, 1_000);
        let hold = reservations.hold("node-a", 2_000).unwrap();
        // 4 slots, 1 guardado: outra requisição entra até 2 em voo (2 + 1 < 4), não com 3
        assert!(hold.admits("req-y", 2, 4));
        assert!(!hold.admits("req-y", 3, 4));
        // a dona sempre entra
        assert!(hold.admits("req-x", 3, 4));
        // nó inteiro: ninguém mais
        reservations.reserve("node-a", "req-x", 5_000, 1_000);
        assert!(
            !reservations
                .hold("node-a", 2_000)
                .unwrap()
                .admits("req-y", 0, 4)
        );
    }

    #[test]
    fn restore_troca_tudo_e_descarta_as_vencidas() {
        let reservations = Reservations::new();
        reservations.reserve("antiga", "r", 5_000, 1_000);
        let entry = |expires| Reservation {
            request_id: "req-z".into(),
            expires_at_unix_ms: expires,
            slots: 2,
        };
        reservations.restore(
            [
                ("viva".to_string(), entry(9_000)),
                ("morta".to_string(), entry(1_500)),
            ],
            2_000,
        );
        let listed = reservations.list(2_000);
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].0.as_str(), listed[0].1.slots), ("viva", 2));
    }

    #[test]
    fn list_drops_expired_holds_and_sorts_by_node() {
        let reservations = Reservations::new();
        reservations.reserve("node-b", "req-x", 5_000, 1_000);
        reservations.reserve("node-a", "req-y", 1_000, 1_000);

        let listed = reservations.list(2_000);
        // node-a's reservation expired at 2_000, so only node-b remains.
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, "node-b");
    }
}
