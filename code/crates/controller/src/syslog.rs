//! Exportação de eventos para um coletor externo (F05): syslog sobre UDP (RFC 5424).
//!
//! `AI_CLUSTER_EVENTS_SYSLOG=host:porta` liga uma tarefa que assina o barramento de eventos e envia
//! cada evento novo como um datagrama. Syslog porque quase todo coletor (rsyslog, syslog-ng,
//! Vector, Loki via Promtail, Graylog) o entende, sem dependência nova nem protocolo próprio.
//!
//! Limites assumidos: UDP não confirma entrega nem criptografa. Os eventos `audit` carregam o IP
//! de quem agiu e saem em texto claro; use uma rede fechada ou um relay local. Um coletor fora do
//! ar só faz o evento ser perdido: o controller não espera por ele.

use std::sync::Arc;

use ai_cluster_protocol::cluster::Event;

use crate::events::EventLog;

/// Facility `local0` (16). A severidade vai de 5 (notice, ações do operador) a 6 (info).
const FACILITY_LOCAL0: u8 = 16;
/// Detalhe truncado para o datagrama não fragmentar numa MTU de 1500 com folga.
const MAX_DETAIL_BYTES: usize = 1000;

/// Data e hora UTC em RFC 3339 com milissegundos, a partir de milissegundos Unix.
/// Sem dependência de calendário: o algoritmo de "dias desde a época para data civil".
fn rfc3339(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let millis = unix_ms % 1000;
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // days_from_civil invertido (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Só ASCII gráfico e sem espaço, até `max` bytes; vazio vira o valor nulo `-` do RFC 5424.
fn token(text: &str, max: usize) -> String {
    let clean: String = text
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(max)
        .collect();
    if clean.is_empty() {
        "-".to_string()
    } else {
        clean
    }
}

/// O datagrama de um evento: `<PRI>1 TIMESTAMP HOST APP PROCID MSGID SD MSG`.
pub fn format_event(event: &Event, host: &str) -> String {
    let severity = if event.kind == "audit" { 5 } else { 6 };
    let mut detail: String = event
        .detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if detail.len() > MAX_DETAIL_BYTES {
        let mut cut = MAX_DETAIL_BYTES;
        while !detail.is_char_boundary(cut) {
            cut -= 1;
        }
        detail.truncate(cut);
    }
    format!(
        "<{}>1 {} {} ai-cluster-controller - {} - {}",
        FACILITY_LOCAL0 * 8 + severity,
        rfc3339(event.at_unix_ms),
        token(host, 255),
        token(&event.kind, 32),
        detail
    )
}

/// Assina o barramento e envia cada evento novo a `target` (`host:porta`). Deve rodar dentro do
/// runtime do tokio. Um alvo que não resolve só é dito uma vez, e a tarefa não sobe.
pub fn spawn(events: Arc<EventLog>, target: String) {
    let host = std::env::var("HOSTNAME").unwrap_or_default();
    let mut bus = events.subscribe();
    tokio::spawn(async move {
        let mut announced_unresolved = false;
        // O alvo é resolvido de novo depois de um erro de envio: um DNS que ainda não respondia na
        // partida, ou um coletor que mudou de IP, não desligam a exportação até o próximo reinício.
        'resolve: loop {
            let Some(addr) = tokio::net::lookup_host(&target)
                .await
                .ok()
                .and_then(|mut addrs| addrs.next())
            else {
                if !announced_unresolved {
                    announced_unresolved = true;
                    println!("AI_CLUSTER_EVENTS_SYSLOG={target} ainda não resolve; tento de novo");
                }
                // Eventos que chegam enquanto o alvo não resolve ficam no barramento (até 256).
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                continue 'resolve;
            };
            announced_unresolved = false;
            let local = if addr.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            };
            let socket = match tokio::net::UdpSocket::bind(local).await {
                Ok(socket) => socket,
                Err(err) => {
                    println!("syslog: não consegui abrir o socket UDP: {err}; tento de novo");
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    continue 'resolve;
                }
            };
            println!("eventos exportados por syslog UDP para {addr}");
            loop {
                let event = match bus.recv().await {
                    Ok(event) => event,
                    // Quem não acompanha perde os mais antigos; segue com os novos.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                };
                let datagram = format_event(&event, &host);
                if let Err(err) = socket.send_to(datagram.as_bytes(), addr).await {
                    println!("syslog: envio falhou ({err}); resolvendo o alvo de novo");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue 'resolve;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, detail: &str, at: u64) -> Event {
        Event {
            at_unix_ms: at,
            kind: kind.to_string(),
            detail: detail.to_string(),
        }
    }

    #[test]
    fn a_data_vem_do_relogio_unix_inclusive_em_ano_bissexto() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_000_000_000_000), "2001-09-09T01:46:40.000Z");
        assert_eq!(rfc3339(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(rfc3339(1_793_128_065_000), "2026-10-27T19:07:45.000Z");
    }

    #[test]
    fn o_datagrama_segue_o_rfc_5424() {
        let line = format_event(&event("node_registered", "orangepi-ab", 0), "ctl");
        assert_eq!(
            line,
            "<134>1 1970-01-01T00:00:00.000Z ctl ai-cluster-controller - node_registered - orangepi-ab"
        );
        // ação do operador sai como notice (5), com a facility local0 (16 * 8 + 5 = 133)
        assert!(format_event(&event("audit", "x", 0), "ctl").starts_with("<133>1 "));
        // host vazio e tipo sem caractere gráfico viram o valor nulo
        let nil = format_event(&event("", "d", 0), "");
        assert!(nil.contains(" - ai-cluster-controller - - - d"), "{nil}");
    }

    #[test]
    fn controle_vira_espaco_e_o_detalhe_e_truncado_sem_quebrar_utf8() {
        let line = format_event(&event("audit", "linha1\nlinha2\x1b[2J", 0), "h");
        assert!(!line.contains('\n') && !line.contains('\x1b'));
        let long = "ã".repeat(900); // 1800 bytes
        let line = format_event(&event("audit", &long, 0), "h");
        let msg = line.rsplit(" - ").next().unwrap();
        assert!(msg.len() <= MAX_DETAIL_BYTES && msg.chars().all(|c| c == 'ã'));
    }

    #[tokio::test]
    async fn cada_evento_novo_chega_ao_coletor_por_udp() {
        let collector = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = collector.local_addr().unwrap().to_string();
        let events = Arc::new(EventLog::new());
        spawn(events.clone(), target);
        // dá tempo de a tarefa resolver, abrir o socket e assinar antes do primeiro evento
        for _ in 0..50 {
            events.record(1_000, "node_registered", "w1".to_string());
            let mut buffer = [0u8; 2048];
            if let Ok(Ok((n, _))) = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                collector.recv_from(&mut buffer),
            )
            .await
            {
                let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                assert!(
                    text.contains("node_registered") && text.ends_with(" w1"),
                    "{text}"
                );
                return;
            }
        }
        panic!("o coletor não recebeu nada");
    }
}
