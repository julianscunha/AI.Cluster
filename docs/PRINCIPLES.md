# Princípios Arquiteturais

## 1. Plano de aplicação separado do plano de computação

Agentes, aplicações e orquestradores ficam fora do cluster. O cluster expõe capacidade por API.

## 2. O cluster é uma plataforma de computação

A abstração interna é workload e Work Unit, não "chatbot".

## 3. Hardware é recurso, não identidade

CPU, GPU, NPU, RAM, armazenamento e rede são recursos mensuráveis.

## 4. Capacidade é empírica

O sistema mede o equipamento e atualiza a capacidade com base em execução real.

## 5. Saúde e capacidade são diferentes

Um node pode ter alta capacidade nominal e estar degradado no momento.

## 6. Confiança e saúde são diferentes

Um node saudável pode estar em quarentena por motivo de segurança.

## 7. Estado deve sobreviver à mudança de topologia

Placement pode mudar; estado lógico deve permanecer recuperável.

## 8. Otimizar antes de ampliar

Primeiro evitar trabalho. Depois executar localmente. Depois utilizar modelos menores. Por fim distribuir modelos grandes.

## 9. Comunicação é custo

Rede e USB são recursos com custo. Recomputation pode ser melhor que transferência.

## 10. Grafos são estruturas de decisão

Grafo é usado quando relações, dependências e proximidade são relevantes; não deve ser usado indiscriminadamente.

## 11. Observabilidade é parte do runtime

Uma decisão sem telemetria não pode ser avaliada.

## 12. Falhas são normais

Node perdido, rede degradada, temperatura elevada e armazenamento com problemas devem ser eventos previstos pelo sistema.

## 13. Não usar todos os recursos é uma decisão válida

Um node pode permanecer disponível e não receber trabalho se o custo adicional superar o benefício.

## 14. Compatibilidade deve ser explícita

Arquitetura, runtime, protocolo, modelo, precisão e capabilities devem ser verificáveis.

## 15. Segurança por padrão

Equipamento descoberto não é automaticamente equipamento confiável.
