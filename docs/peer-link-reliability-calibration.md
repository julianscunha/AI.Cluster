# Calibração da confiabilidade dos enlaces Worker-to-Worker

## Objetivo

Validar se a estimativa de custo do caminho Worker-to-Worker representa o comportamento observado em transferências reais antes de alterar os pesos de confiabilidade do Transfer Planner.

Esta etapa é de calibração, não de ajuste arbitrário de pesos. Os resultados devem ser reproduzíveis e comparados com uma referência sem penalidade de confiabilidade.

## Métricas e semântica

- **RTT**: latência de ida e volta observada pela sonda gRPC.
- **Jitter**: variação de RTT calculada pelas amostras da sonda.
- **Throughput da sonda**: vazão do payload echo da sonda; usar como fallback quando não houver throughput de transferência real válido.
- **Throughput real**: bytes concluídos divididos pela duração observada da transferência; priorizar esta métrica para estimar o tempo de transferência.
- **Taxa de falha de transferência**: (falhas + timeouts) / tentativas reais de transferência no intervalo de agregação.
- **Taxa de falha da sonda**: falhas, timeouts e resultados inválidos / tentativas de sonda. Só deve ser usada como fallback quando ainda não houver tentativas reais de transferência.
- **Perda de pacotes IP**: métrica distinta; não inferir perda de pacotes a partir de falhas de RPC gRPC.

O custo atual baseado em métricas medidas pode ser expresso como:

`custo_us = ceil(RTT + jitter) + ceil(tempo_transferência_estimado / clamp(1 - taxa_falha, 0.25, 1.0))`

A fórmula é uma heurística de custo esperado, não uma garantia de latência nem uma estimativa estatística calibrada de probabilidade de sucesso.

## Matriz de cenários

Executar cada cenário com os mesmos tamanhos de payload e número de repetições para todos os enlaces comparados. Registrar o resultado bruto e as métricas agregadas.

| Cenário | Condição controlada | Resultado esperado |
|---|---|---|
| Referência estável | Baixo RTT/jitter, sem falhas | Throughput real domina o custo; penalidade de confiabilidade próxima de 1 |
| Latência alta | Aumentar RTT sem degradar throughput | Custo aumenta pela parcela de latência, sem inflar a taxa de falha |
| Jitter alto | Variar latência entre sondas | Custo aumenta moderadamente; throughput não deve ser confundido com jitter |
| Throughput reduzido | Limitar a vazão do enlace | Estimativa de duração acompanha o throughput real, quando disponível |
| Falhas intermitentes | Injetar falhas de transferência com sucesso entre elas | Taxa de falha acompanha as tentativas reais e aumenta o custo |
| Timeouts | Atrasar respostas além do prazo configurado | Timeouts entram no numerador e no denominador das tentativas |
| Sonda indisponível, transferência válida | Fazer a sonda falhar e concluir transferências reais | Histórico serve de fallback; taxa real de transferência tem precedência |
| Transferência indisponível, sonda válida | Sonda funciona, transferências ainda não observadas | Taxa de falha da sonda é usada como fallback |
| Histórico reiniciado | Reiniciar o Controller com histórico persistido | Métricas válidas são recuperadas e amostras antigas são descartadas conforme a janela |
| Amostras insuficientes | Apenas uma ou duas tentativas | Planner não deve tratar a taxa observada como certeza; registrar o tamanho da amostra na análise |

## Procedimento

1. Fixar versão do Controller/Worker, configuração, tamanho de payload e número de repetições.
2. Capturar cada tentativa: enlace direcionado, timestamp, bytes, duração, sucesso/falha/timeout e identificador do cenário.
3. Para cada enlace, calcular independentemente throughput agregado, RTT/jitter, tentativas, taxa de falha e custo previsto.
4. Comparar o custo previsto com a duração observada de transferências concluídas. Separar tentativas malsucedidas da duração de transferências bem-sucedidas.
5. Comparar ranking de enlaces com a referência sem penalidade e com o custo medido; verificar se a penalidade evita enlaces instáveis sem descartar enlaces confiáveis apenas por serem mais lentos.
6. Repetir o experimento pelo menos três vezes e registrar mediana e dispersão. Não mudar pesos com base em uma única execução.

## Critérios de aceite

- A taxa de falha de transferências é calculada somente sobre tentativas de transferência, sem misturar sondagens.
- A taxa de falha da sonda é usada apenas quando não existem tentativas reais de transferência na janela.
- O throughput real válido prevalece sobre o throughput da sonda.
- Nenhuma taxa calculada fica fora do intervalo `[0, 1]`; ausência de amostras é distinguida de uma taxa comprovada de zero.
- Enlaces com falhas induzidas recebem custo maior que o mesmo enlace sem falhas, mantendo constantes as demais métricas.
- O aumento de RTT/jitter eleva o custo de forma monotônica; a redução do throughput também eleva o custo.
- A retomada por offset e a validação SHA-256 não geram contagens duplicadas de sucesso para uma única transferência lógica.
- Os testes automatizados cobrem falhas, timeouts, amostras insuficientes, fallback histórico e reinicialização.
- O relatório de experimento documenta limitações, tamanhos de amostra e configuração; não apresenta falhas gRPC como perda de pacotes IP.

## Risco de retenção compartilhada

O histórico atual mantém até 32 observações por enlace, compartilhadas entre sondas e transferências. Cargas com muitas sondas podem expulsar observações de transferência antes de outras observações, e vice-versa. Durante a calibração, verificar se isso distorce a janela efetiva. Se ocorrer, considerar limites separados por tipo de observação ou uma política de retenção por tempo e tipo antes de calibrar os pesos.

## Saída da etapa

Publicar um relatório com configuração, resultados brutos, tabelas por enlace, comparação entre custo previsto e duração observada, e decisão explícita sobre manter ou alterar a fórmula. Só então calibrar pesos de confiabilidade e prosseguir para cancelamento cooperativo e replanejamento durante a execução.
